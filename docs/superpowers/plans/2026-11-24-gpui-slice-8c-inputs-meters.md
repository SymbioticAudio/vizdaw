# 27.1.0 Slice 8c: Track Inputs, Monitoring, Meters, the Growing Take and the Markers Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A track can be given an input and a monitor mode, and shows its input meter, in the GPUI app, as in the Swift app. The track header gets the monitor button (`A` / `I` / `–`), the input picker and the input meter with its clip mark. The mixer strip gets its input picker. Every audio region on the arrangement draws its waveform. The take draws itself in the same way while it records, so it looks the same during and after recording, and the right pane's Markers view lists the markers and set locators. `lane.set_input` and `lane.set_monitor` work on the channel. `state.get input` keeps its own peak hold, apart from the meters'. After 8c, `gpui_arms_meters_counts_in_and_draws_the_take` passes on the Mac and on Linux with the offline device (fake input). Takes (choose/rename/delete) come in a later slice.

**Architecture:** A new `vizdaw-app` model `LaneInputs` (Swift `M/LaneInputs.swift`) sits on the document. It holds the picker sources (read off the main thread), the meters (read in the poll) and the opening note. `RegionWaves` (Swift `MediaModel.waveform`, `E/RegionDrawing.swift`, `WaveformGeometry`) reads each visible region's `region_waveform` off the UI thread into a cache. `GrowingTake` (Swift `M/GrowingTake.swift`) computes the take's pieces, and the record poll reads their waveform rows. Render reads only held data, and one drawing function draws both. `MarkerList` (Swift `M/PaneContent.swift:191-229`) builds the Markers pane's rows. GPUI draws these in `track_headers.rs`, `mixer_view.rs`, `arrangement.rs` and `panes.rs`. The picker is an anchored menu, following the » menu's `deferred(anchored())` pattern, and it shows in `ui.tree`. `audio_input_channels` and `midi_source_ports` join the seam (8a Task 1 left them off).

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. Parity rows: `docs/superpowers/specs/2026-11-22-parity-checklist.md` ("### Track" `lane.set_input`, `lane.set_monitor`; e2e `arms_meters_counts_in_and_draws_the_take`, `window_views_and_panes_over_the_channel` markers step; "Linux gaps" live input row). Swift reference with exact strings and file:line: the slice 8 inventory (`slice8-inventory.md` §2.1–2.3, §3, §5 rows `LaneInputsTests`, `RecordUiCarryTests`, `GrowingTakeTests`, §7 row "8c", §8.3, the 8c list in §8.4). Plans 8a and 8b Outcomes (every 8c carry is a step below). Swift sources are still in the tree for reading: `app/Sources/VizDAWAppModel/{LaneInputs,GrowingTake,PaneContent}.swift`, `app/Sources/VizDAWEditors/{LaneHeaderView,ArrangementLayout,ArrangementView,RegionDrawing,RegionEditing}.swift`, `app/Sources/VizDAWAppModel/MediaModel.swift:269-277`, `app/Sources/VizDAW/{StripInputView,PaneViews}.swift`, tests in `app/Tests/VizDAWAppModelTests/`.

## Global Constraints

- All slice 7a–8b Global Constraints apply:
  - The Swift app is retired: parity with its behaviour, no compatibility.
  - Exact Swift strings, with the inventory's file:line.
  - The UI says "track", never "lane" (core identifiers may say lane).
  - No banned vendor/DAW product names in repo text.
  - Nothing slow on the UI thread. Picker sources (`midi_source_ports`, `audio_input_channels`) are read on the background executor with a generation guard. Meter peaks and growing-take waveform rows are read in the poll, never in render.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend (`VIZDAW_AUDIO=offline`, `VIZDAW_AUDIO_INPUT=fake`, MIDI by `midi.inject`). Every 8c test and e2e must pass there.
- e2e on the Mac: build `cargo build -p vizdaw-gpui`, then run with `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored <name>`. `start_with` already sets the fake input when asked.
- Slice 8 rulings binding 8c:
  - Takes (choose/rename/delete, the header's `Takes ▸`) come in a later slice.
  - The Customize Transport Bar sheet and `audio.set` stay with the Settings slice. The e2e port skips Swift's `audio.set` steps (`buffer_frames`) and says so in a comment.
  - Live audio/MIDI input on Linux is H3. Off macOS, `audio_input_channels()` answers `None` and `midi_source_ports()` answers empty. The picker then offers two unnamed channels before the engine opens, then the fake input's three. That is expected, not a bug.
  - The opening status (Swift `openingNote`): macOS shows `Opening the audio input… If macOS asks for microphone access, answer its prompt.`; other platforms show `Opening the audio input…`, with the microphone sentence omitted. This is no new string: the Linux text is the first sentence of Swift's.
  - Surfaces are not in the GPUI app yet. The claimed-ports hook (`(used by a surface)`) is on the model and empty until they come.
- `state.get input` never reads peaks straight from the session. It reads `LaneInputs::take_channel_levels()`. One core read (which resets the core's peaks) feeds both holds, so the meters and the channel each see every peak (8a Task 6 carry).
- Input and monitor changes are lane edits, one undo step each: `Set Input`, and the monitor's step as the mixer's `cycle_monitor` names it today. While recording, the core answers Busy, and the status line says so.
- Look: the meter and clip colours, the monitor button's per-mode tint (Swift `StripPalette.monitor`) and the take's fill are theme tokens, not literals. Show-finished-UI rule: screenshots go to the owner only once the header and the take resemble the approved look. The controller runs Linux over ssh and makes the on-screen macOS checks; every look item gets a manual-check row.
- Review pace: fix crash, data loss and wrong overwrite; log other edge cases to the backlog.
- Owner ruling (2026-11-24): audio regions draw waveforms in 8c, for Swift parity, so a take looks the same during and after recording.
  - Swift's behaviour (`ArrangementView.swift:264-290`, `RegionDrawing.swift`, `RegionEditing.swift:196-230`, `MediaModel.swift:272-277`):
    - The core picks the overview level. `region_waveform(event, from_tick, to_tick, columns)` is asked for the region's visible ticks at one column per point, at most 8192 columns. It uses the 256-frame peaks, or exact samples below 256 frames per column, which answer `Coarse` until they are read.
    - The core applies the region's gain and fades. The app does not.
    - Channels are stacked top to bottom (mono one band, stereo two). Each column is a bar from max to min around its band's centre, clipped to ±1. Silence and non-finite values draw a hairline.
    - Bars are drawn under the gain band in the region-line colour at 0.65 alpha, only for `Ready` or `Coarse`. `Pending` draws nothing and is asked again after `IdleUpdate.overviews_changed`.
    - An offline region is hatched with an `Offline` pill and is never asked for.
  - The GPUI app adds what Swift did on the main thread: the reads run on the background executor with a generation counter (the 7a pattern, `crates/vizdaw-app/src/aaf.rs` `generation`). Rows are cached per region, media and zoom (ticks per point) and visible slice. The cache is dropped on media change (`media_changed`), overview news, a snapshot that changes the region, or a zoom change.

## Review Focus

1. Peaks: each core read feeds both holds; neither the meters nor `state.get input` loses a peak to the other; the meters still read while the transport read fails (Swift `metersPollWhileTheTransportReadFails`).
2. Poll: fast while any shown track meters, even when stopped and not recording (`record_polls_fast`). Never fast with no meter (`noMeterNoFastPoll`). Meters, levels and clip marks clear when the last meter goes, and so does the opening note (only if the status is still that note).
3. Off-main reads: an older, slower source read never overwrites a newer one (the generation guard). A new document's first picker lists the sources warmed at launch. No `midi_source_ports` or `audio_input_channels` call runs on the UI thread.
4. The growing take: nothing is drawn before the roll or in another sequence. A saturated file frame draws nothing and never traps (no overflow or panic in tick/frame maths). At most 32 pieces and 8192 columns. The take is not a Locate during the pass.
5. Region waveforms: no `region_waveform` call on the UI thread or in render; an older read never replaces a newer one; offline media is never asked; `Pending` asks again only after overview news (no busy loop).
6. The picker: choosing the current input is a no-op (no undo step); a choice while recording shows Busy and changes nothing; a return track keeps its synth (`return_of`).

---

### Task 1: picker sources and Software Monitoring on the seam

**Files:** `crates/vizdaw-app/src/session.rs` (`CoreCalls`), `crates/vizdaw-app/src/testing.rs` (fake core); tests `crates/vizdaw-app/tests/session_record.rs`.
**Produces:**
- `CoreCalls::audio_input_channels() -> Option<InputChannelsRow>` and `CoreCalls::midi_source_ports() -> Vec<String>`, calling `core-ffi` `host.rs:1222, 1258` (the 8a Task 1 carry).
- The fake core scripts both and `audio_settings()` (`software_monitoring`). This lifts 8a Task 1's "the fake core can't script audio_settings" cost. It also counts calls, and can hold a read until released, so tests can show a read is off-main and out of order.
- [ ] Tests:
  - The real core answers on the Mac: a list, possibly empty, without panicking. On Linux it answers `None` and empty.
  - The fake's scripted values and its held read.
  - Then implement and commit `app: input sources and software monitoring on the seam`.

### Task 2: the `LaneInputs` model, the channel's input levels and the fast poll

**Files:** `crates/vizdaw-app/src/lane_inputs.rs` (new; Swift `M/LaneInputs.swift`), `crates/vizdaw-app/src/{document.rs,record.rs,lib.rs}` (an `inputs` per document; the poll; `record_polls_fast`), `crates/vizdaw-app/src/mixer/model.rs` (`next_monitor` / `monitor_help` move to `lane_inputs`, and the mixer reuses them), `crates/vizdaw-app/src/commands.rs` (`lane.set_input`, `lane.set_monitor`), `crates/vizdaw-gpui/src/control.rs` (`state.get input`, the two lane ids), `crates/vizdaw-gpui/src/app_state.rs` (warming the sources at launch); tests `crates/vizdaw-app/tests/lane_inputs.rs`, `crates/vizdaw-gpui/tests/control.rs`.
**Produces:**
- Picker groups (`:146-181`):
  - Untitled: `No Input`.
  - `Audio`: one entry per known channel, `In k (<name>)`, or `In k` when the name is empty, then the pairs `In 1+2`, … Channels are known from the engine's open input (`input_info`, channels > 0), else from the device read without the engine, else there are 2.
  - `MIDI Input`: `All MIDI Inputs`, then each source, as `<p> (used by a surface)` when a surface has claimed it.
  - `MIDI Channel`: `Any Channel`, `Channel 1`…`Channel 16`.
  - Which groups show follows `kinds(lane)`: by input, else by output/destination, else both.
- Titles (`:191-225`):
  - `title(input)`: `No Input`, `In 3`, `In 3+4`, `<port or "All MIDI">[ · Ch N]`.
  - `title_of(lane)` gives `<port> · MPE` with MPE on.
  - `picker_title(lane)` leads with `<track>: ` when another track plays through the same path.
  - `badge` is `⇠ <synth>`, from `return_name`, or `Synth` when the destination is unknown.
- `choose(choice, lane)`: a lane edit `SetInput` (undo `Set Input`); no edit when unchanged. `cycle_monitor`: Auto → Input → Off; `monitor_label` gives `A` / `I` / `–`; `monitor_help` gives Swift's three texts.
- Sources:
  - `refresh_info()` reads `input_info` (a picker is opening) and starts `refresh_sources` on the background executor. The device is read only while the engine's input is closed.
  - A generation guard keeps a newer read from being overwritten.
  - `warm_sources()` runs once at launch and seeds every new document (Swift `lastSources` / `lastDevice`).
- Meters (`:294-406`):
  - `shows_meter(lane)`: an audio input, armed or (Software Monitoring on and monitor Input). `metering()`: any shown track meters.
  - `poll_meters(now)`:
    - It reads the opening flag (`set_opening`, never over another status) and the core's peaks once into `meter_held` and `channel_held` (`merged`, sanitised).
    - Each metered track takes the louder of its channels against the decayed last level (`MixerMath::METER_DECAY`).
    - A clip mark lasts 2 s at a peak ≥ 1, or when the record state's `clipped` count grows. The clock is injectable.
    - With no metered track, it clears levels, clips, holds and the opening note.
  - `refresh_monitoring()` reads Software Monitoring from `audio_settings()`.
- The poll (`document.rs` poll / 8a's record poll):
  - `poll_meters` runs on every tick while `metering()`, even when the transport read fails.
  - `record_polls_fast()` becomes `is_recording() || counting_in || inputs.metering()` (the 8a Task 3 carry: the place 8a left for it).
- `take_channel_levels()` feeds `state.get input`'s `levels` (the 8a Task 6 carry). `info` stays the session's.
- The opening note is the platform text from Global Constraints (`cfg!(target_os = "macos")`).
- On the channel:
  - `lane.set_input` (`SET_INPUT_PARAMS`: `kind` none|audio|midi, channel, stereo, port, MIDI channel) and `lane.set_monitor` (`mode` auto|input|off) run as lane edits.
  - Hostile or mismatched params answer `invalid_params` (an audio channel past the known count is the core's to refuse).
  - Busy while recording is `busy`.
- [ ] Tests (Swift `LaneInputsTests`, `RecordUiCarryTests` model rows, `RecordModelTests` lane rows):
  - `audioChoicesNameTheDevicesChannels`, `thePickerListsTheDevicesChannelsBeforeTheEngine`, `aReturnLaneKeepsItsSynth`, `midiChoicesAreSourcesAndChannels`, `aSurfacesPortIsMarkedInThePicker`, `aSharedPathsPickerNamesItsLane`.
  - `theSourceListIsReadOffTheMainThread` (the fake core's held read: the model answers before it is released, and an older read released last changes nothing) and `aNewDocumentsFirstMenuListsTheWarmedSources`.
  - `theMonitorButtonCyclesAutoInputOff` (and the mixer's cycle still passes).
  - `metersShowArmedOrMonitoredLanesAndHoldTheClip`, `noMeterNoFastPoll`, `theMetersAndTheChannelEachSeeEveryPeak`, `softwareMonitoringOffHidesAnInputMonitoredMeter`, `softwareMonitoringReachesTheMetersAtOnce`, `metersPollWhileTheTransportReadFails`.
  - `anOpeningInputSaysSo` (each platform's text), `openingIsClearedOnDisarm`, `theInputHolderIsTheFirstSessionWithADevice`.
  - Poll fast while an armed track meters and the transport is stopped and not recording; slow once it is disarmed.
  - Channel: `laneInputsAndMonitorsAreLaneEdits` (undo names), `hostileLaneParamsAreInvalid`, `laneInputParamsMustFitTheKind`; `state.get input` and the meters both see a scripted peak; Busy while a scripted pass records.
  - Then implement and commit `app, gpui: track inputs, meters and the channel's input levels`.

### Task 3: the track header's monitor button, input picker and meter, and the strip's picker

**Files:** `crates/vizdaw-gpui/src/track_headers.rs` (monitor toggle before M S R; the input row; meter and clip mark; `header()` words), `crates/vizdaw-gpui/src/input_picker.rs` (new: the anchored grouped menu, shared by headers and strips), `crates/vizdaw-gpui/src/mixer_view.rs` (the strip's input row: picker + existing monitor); tests `crates/vizdaw-gpui/tests/{tracks.rs,mixer.rs}`.
**Produces:**
- The header (`E/LaneHeaderView.swift:200-240, 288, 341, 545-580`):
  - A monitor button with `monitor_label` and the per-mode tint, on unless Off, and help/a11y `monitor_help`. A click calls `cycle_monitor`.
  - The input row `▾ [⇠ badge · ]<title>`, truncated in the middle. A click opens the picker (`refresh_info` first), with Swift's sections and titles and a ✓ on the current input. A choice calls `choose`.
  - The input meter when `shows_meter`, hidden when the row is under 46 pt (Swift `LaneHeaderLayout.showsMeter(laneHeight:)`), with the clip mark lit while `is_clipped`.
- `header()` a11y (`E/ArrangementLayout.swift:177-192`, `E/LaneHeaderView.swift:677-679`) adds, in order:
  - `input <title_of>` when there is an input.
  - `recording the return of <synth>`.
  - `monitoring input` / `monitoring off`.
  - `, input opening` while opening and metered.
  - The comment saying "input words come later" goes.
- The mixer strip's input row (`V/StripInputView.swift:8-48`): the picker shows `picker_title`, with a11y `Input of <track>` and help `Input of <track>, recording ⇠ …` when there is a badge. The monitor button's a11y becomes `<track>: <help>` (Swift), not `Input monitor <strip>`. Update the tests that read the old label.
- A picker closes on Esc, on a click outside, or on a choice; one is open per window.
- [ ] Tests:
  - The tree names the header's monitor button, input row and meter as Swift does; `theMonitorButtonHasItsTooltip`; `theMeterHidesUnder46PointsAndTheMonitorClickCycles`; `monitorTooltipsCoverTheVisibleRowsAndFollowAScroll`.
  - The header words for each input, return and monitor case, and `, input opening`.
  - The picker: sections in order, ✓ on the current input, a choice makes one undo step, choosing the current one makes none, and a choice while recording shows Busy in the status line.
  - The meter follows scripted peaks, and the clip mark lights at 1.0 and goes after 2 s on the test clock.
  - Mixer: the strip picker's title with a shared path, `Input of <track>`, and the monitor's a11y.
  - Then implement and commit `gpui: monitor button, input picker and meter on the track headers and strips`.

### Task 4: waveforms on audio regions

**Files:** `crates/vizdaw-app/src/session.rs` and `testing.rs` (`region_waveform` on the seam; `IdleNews.overviews_changed`), `crates/vizdaw-app/src/region_waves.rs` (new: requests, cache, generation counter), `crates/vizdaw-app/src/document.rs` (the cache per document; overview and media news), `crates/vizdaw-gpui/src/region_drawing.rs` (new: `waveform_bars`, as Swift `WaveformGeometry.bars`, plus the hatch and pill), `crates/vizdaw-gpui/src/arrangement.rs` (the regions of each audio block drawn and in the tree); tests `crates/vizdaw-app/tests/region_waves.rs`, `crates/vizdaw-gpui/tests/arrangement.rs`.
**Produces:**
- `DocSession::region_waveform(event, from_tick, to_tick, columns) -> OverviewRows`. The fake scripts rows and states per event, counts calls, and can hold a read.
- `IdleNews.overviews_changed` carries the core's `IdleUpdate` flag.
- `RegionWaves::want(region, media, visible ticks, width)` gives the request: the visible ticks, columns = width in points rounded up, at most 8192.
  - It asks nothing for offline media (`media` rows' offline flag), an empty slice or zero width.
  - It runs the read on the background executor, tagged with the generation. A reply older than the newest is dropped.
  - Rows are held by (event, media, ticks per point, slice). `Pending` is kept as pending and asked again after `overviews_changed`; `Coarse` is drawn and asked again after it too.
  - The cache is dropped on `media_changed`, on a snapshot that changes the region (gain, fades, offset, length), and on a zoom change.
- Drawing (`E/RegionDrawing.swift` `draw`): the body as today, then, clipped to it:
  - An offline region is hatched with an `Offline` pill when there is room.
  - Otherwise the waveform bars, for `Ready` or `Coarse` rows, in the wave rect under the gain band, in the region-line colour at 0.65 alpha (a theme token).
  - Then the fade lines and the outline as today.
- Each drawn audio region is a tree element labelled as Swift `RegionA11y.label` (`Region <name> on <track>, bar(s) …, gain …[, crossfade][, fades][, offline][, muted]`). Its value is the waveform state: `ready`, `coarse`, `pending`, `offline` or `none`.
- The arrangement redraws when rows arrive.
- [ ] Tests:
  - Model: the request per zoom (columns follow the visible width and cap at 8192; a zoom change asks again at the new ticks per point, a scroll at the same zoom asks only for the new slice).
  - Model: offline media asks nothing and is `offline`; `Pending` then `overviews_changed` asks again once; `media_changed` drops the cache.
  - Model: an older held read released after a newer one changes nothing (generation); no read runs on the calling thread (the fake's held read).
  - Geometry: Swift `WaveformGeometry` rows: mono and stereo bands, clipping at ±1, a hairline for silence and non-finite values, short arrays draw nothing.
  - GPUI: an audio region's tree element carries `pending`, then `ready` after the scripted read and a redraw. An offline region reads `offline` with its label `…, offline`, and no read was made. Render makes no `region_waveform` call (call count unchanged across frames).
  - Then implement and commit `app, gpui: waveforms on audio regions`.

### Task 5: the take drawn while it records

**Files:** `crates/vizdaw-app/src/growing_take.rs` (new; Swift `M/GrowingTake.swift`), `crates/vizdaw-app/src/record.rs` (the waveform rows read in the poll), `crates/vizdaw-gpui/src/arrangement.rs` (drawing the pieces); tests `crates/vizdaw-app/tests/growing_take.rs`, `crates/vizdaw-gpui/tests/arrangement.rs`.
**Produces:**
- `growing_takes()`: one take per armed audio track recording in the shown sequence. It runs from the later of pass start and range start to what is written, at most to punch-out. It is placed by the track's return latency, and its ticks follow the tempo map. It uses saturating maths throughout.
- `slice(rect, visible, from, to)` gives the visible frames and columns (≤ 8192 shared by the pieces). Pieces are 64 pt wide, at most 32.
- The record poll asks `recording_waveform(lane, from, to, columns)` for each piece, using the arrangement's last visible rect, which the view hands over on layout. It keeps the rows by lane. Render draws only held rows.
- A failed read keeps the last rows; the rows go at Stop.
- The arrangement draws the take with Task 4's region drawing (`waveform_bars`, the same colours and wave rect), in a body in the record colour, so it becomes the same waveform at Stop. It is drawn under the playhead, with a11y `Recording take` (a new label, not a Swift string; Swift's view had none). The tree shows it, so the e2e can wait for it. Name the label in the plan's Outcome.
- The take is not a Locate: no `transport.locate` runs during the pass.
- [ ] Tests (Swift `GrowingTakeTests`, 9):
  - `theTakeRunsFromTheRangeStartToWhatIsWritten`, `thePunchOutEndsIt`, `nothingGrowsBeforeTheRollOrInAnotherSequence`, `aReturnLaneIsPlacedByItsReturnLatency`, `aNegativeAnchorStartsLater`, `aSaturatedFileFrameDrawsNothingAndNeverTraps`, `theArrangementDrawsItFromTheLivePeaks`, `aGrowingTakeFollowsATempoChange`, `theVisibleSliceAsksForItsFramesAndAtMost8192Columns`.
  - GPUI: with a scripted pass and `FakeSession::set_recording_waveform`, the tree shows `Recording take` on the armed track with a width that grows across two polls, and it is gone after Stop.
  - Render makes no session call (the fake counts `recording_waveform` calls per poll, not per frame).
  - At Stop the take turns into the same waveform: with the same scripted peaks for `recording_waveform` and the placed region's `region_waveform`, the bars drawn for the take's last poll and for the region after Stop are the same (the same `waveform_bars` output for the same rect), and the region's tree element reads `ready`.
  - Then implement and commit `app, gpui: the take drawn while it records`.

### Task 6: the Markers pane

**Files:** `crates/vizdaw-app/src/marker_list.rs` (new; Swift `MarkerList`), `crates/vizdaw-gpui/src/panes.rs` (the `Markers` view replaces its note), `crates/vizctl/tests/e2e_gpui.rs`; tests `crates/vizdaw-app/tests/marker_list.rs`, `crates/vizdaw-gpui/tests/panes.rs`.
**Produces:**
- `MarkerList::rows(info, marks)`: the sequence's markers (`m<i>`, their names) and the set locators (`l<N>`, `Locator N`). They are sorted by tick, then marker before locator, then order. Positions are `bar.beat.unit` by the sequence's meter.
- `go(row)` moves the transport there. A failure sets the status `Go to Marker Failed: <error>`.
- Empty: `No markers or locators.` (`V/PaneViews.swift:201`).
- The pane redraws when the snapshot's markers or the layout's locators change.
- Each row shows its name and position, is a button labelled with its name, and moves on a click.
- [ ] Tests:
  - Swift `RightPaneTests` `markersListTheSequencesMarkersAndSetLocators` and `choosingAMarkerMovesThePlayhead`; the sort's tie rule (marker before locator at one tick); a failed move shows the status.
  - GPUI: the empty note, rows after `marker.add` and `locator.set_2`, and a click that moves the tick.
  - e2e: extend `gpui_window_views_and_panes_over_the_channel` step 4 (the checklist's "markers steps later"). With the right pane on Markers: the empty note, then `marker.add` at bar 3 named `Verse`, then a `ui.click` on `Verse`, after which `state.get transport` `tick` is at bar 3.
  - Then implement and commit `app, gpui: the Markers pane`.

### Task 7: e2e `gpui_arms_meters_counts_in_and_draws_the_take`

**Files:** `crates/vizctl/tests/e2e_gpui.rs`, `docs/superpowers/specs/2026-11-22-parity-checklist.md`.
**Produces:**
- A port of Swift `arms_meters_counts_in_and_draws_the_take` (`X/e2e.rs:1408`) with the same fixture: track `Mic` on `Audio 1`, fake input mono channel 1, monitor Off, 8 bars locked, opened by `app.open`, and `start_with` taking `VIZDAW_AUDIO_INPUT=fake`. In order:
  1. `transport.arm_previous`, then `state.get input` until `levels[0] > 0.2`: `channels` 3, three levels, `opening` false, transport not playing.
  2. The `Mic` header in `ui.tree` shows its input meter, and its label holds `input In 1, monitoring off`.
  3. The `audio.set` steps are skipped (Settings slice), with a comment saying so.
  4. `record.set {count_in: 1, loop: false, replace: true}`, then `transport.record`: `counting_in` true, `playing` false, tick 0, and `record` gives `recording` true with `frames` 0.
  5. `wait_for_record` until `frames ≥ 48_000` and the peak is > 0.2: the peak is ≈0.251 (±0.01) and `pass_start_tick` is 0. The tree shows `Recording take` on `Mic`.
  6. `transport.record` stops it. Then `wait_for_undo` `Record`, and the media item's `peak` is ≈0.251. `Recording take` is gone, and the placed region's tree element on `Mic` reads `ready` (Task 4).
- The test runs on Linux CI as it is: fake input, no device.
- Checklist:
  - Tick `arms_meters_counts_in_and_draws_the_take` "S8c" on Mac and Linux, with the `audio.set` steps noted as waiting for Settings.
  - Tick `lane.set_input` and `lane.set_monitor`, and the `window_views_and_panes` markers step.
  - The Linux-gaps live-input row stands: it gains "the picker offers two unnamed channels until H3".
- [ ] Write the test, see it fail at step 2 or 5 before Tasks 3–5 if you run it out of order, run it green on the Mac (env in Global Constraints) and on Linux, then commit `e2e: gpui arms, meters, counts in and draws the take`.

### Task 8: manual checks, checklist and backlog

**Files:** `docs/testing/manual-checks.md`, `docs/superpowers/specs/2026-11-22-parity-checklist.md`, `docs/feature-backlog.md`.
**Produces:**
- A manual-checks section "Track inputs, meters, the growing take and markers (GPUI slice 8c)", with Mac and Linux rows for the look:
  - The monitor button's letters and tints.
  - The input row's truncation and badge.
  - The picker's sections and its anchoring in the header and the strip.
  - The meter's fall and the 2 s clip mark.
  - The meter hiding on short rows.
  - Region waveforms: mono and stereo bands, the colour against each theme's region fill, gain and fades shaping them, `Coarse` sharpening when zoomed in close, `Pending` filling in, offline hatching with its pill, and no hitch when scrolling a long project.
  - The take's colour, peaks and growth against the playhead, and that at Stop it turns into the same waveform on the placed region.
  - The Markers pane rows and click.
  - On the Mac: a real input device (channel names, `Opening the audio input… If macOS asks…` with the microphone prompt) and a real MIDI source in the picker. These are owner or controller rows. Linux real input is H3.
- Checklist ticks for the rows that pass.
- Backlog:
  - A row "GPUI slice 8c: review leftovers".
  - The claimed-ports hook waiting for surfaces.
  - The output/destination pickers in the GPUI header, if they are still missing (Swift `→ <output>` / `▾ <destination>`; check the checklist first, and do not port them here).
- The controller runs the Linux GPUI tests and e2e over ssh and makes the macOS on-screen checks. Screenshots go to the owner only when the header and the take resemble the approved look.
- [ ] Commit `docs: slice 8c manual checks, checklist and backlog`.
