# VizDAW Sub-project 5: Recording and audio tracks — Design

Status: approved in conversation on 2026-10-18 (scope, approach B and plans 5a–5f). The user reviewed the written spec on 2026-10-18 and asked for three changes, which are now included: the project audio format (§4.1), tracking a hardware synth (§6.7), and MIDI input effects (§6.8).
Parent spec: `2026-10-15-vizdaw-design.md`. Its requirements are binding here and are not repeated in full; this document says how they are built and in what order:
- §5: media pool, tracks, takes, audio regions and undo of recording passes.
- §6: recording, and CoreAudio I/O.
- §8: audio files and the external sample editor.
- §10: offline media.
- §11, step 5.

This sub-project also takes in MIDI timeline recording, which no earlier sub-project owned, automation recording, and MIDI input effects (D26h).

## 1. Scope

In scope:
- **Audio playback.** Importing audio files into the project, a CAF reader and writer, disk streaming, and audio regions sounding through the mixer with their gain and fades.
- **Regions in the editors.** Waveform overviews, fade and gain handles, the media pool window, offline media and relinking.
- **Recording.**
  - Audio and MIDI recording onto armed lanes: replace or overdub, punch, and loop recording into takes, with keep/discard of loop passes.
  - Pre-roll and count-in.
  - Automatic latency compensation.
  - Each recording pass becomes one undo step.
  - Tracking a hardware synth as audio: in one pass, a MIDI lane drives an external synth and an armed audio lane records its return, aligned to the MIDI timeline (§6.7).
- **Project audio format.** Every project has a sample rate and bit depth, chosen when it is created, and stores all of its media in that format (§4.1).
- **MIDI input effects (D26h).** A per-lane chain of built-in effects (arpeggiator, transpose, velocity, chord, input quantize) for live input and thru. What is recorded is the chain's output (§6.8).
- **Monitoring.** Software input monitoring (Auto, Input or Off per lane), auto-input, and MIDI thru.
- **Automation recording.** The Write, Touch, Latch and Trim modes, written from:
  - the on-screen mixer;
  - plugin parameters, including moves made in a plugin's own window;
  - MIDI learn;
  - control surfaces.
- **Control surfaces.** MIDI learn, plus the Mackie Control and HUI protocols, with touch-sensitive motor faders, banks, scribble strips and meters.
- **Bounce/export.** A mixdown or stems, rendered offline (faster than real time) to a file, optionally placed back into a lane.
- **External sample editor** (main spec §8): the editor gets a copy, VizDAW watches it and relinks the region when it changes.

Out of scope:
- **Freeze tracks.** Deferred; it will reuse the bounce renderer.
- **OMF/AAF and SMF import** (sub-project 6). This sub-project's `MediaImporter` and media commands are what those importers will call.
- **Offline (destructive) DSP.** The user prefers non-destructive processing (D26e). Normalize is offered as region gain (§4.4).
- **Time-stretching and varispeed.** A region always plays at its recorded speed, starting at its mapped tick.
- **Automatic crossfades.** Overlapping regions follow the parent rule (the later start takes over). Users shape the joins with fades.
- **Plugin and third-party MIDI input effects, and groove.** Only the built-in set of §6.8 is included.
- **Real-time bounce and an external-instrument round trip.** This means an "external instrument" insert that sends MIDI to hardware and returns its audio live, so that bounce and freeze could include hardware synths. It is deferred to a later sub-project. Until then, hardware synths are captured by tracking them as audio (§6.7), and offline bounce leaves external MIDI silent (§9.1).
- **Disk-space management beyond a warning**, and network/remote media.

## 2. Decisions (user, 2026-10-18)

- **Scope.** Recording covers MIDI and audio together. Both share one record engine with punch, loop passes, takes, replace/overdub, and one undo step per pass.
- **Media.** Imported files are converted on import into CAF in the project's format, inside the package's `Media/`. The original file is never changed.
- **Project format (review).** A project has a sample rate and a bit depth (16-bit integer, 24-bit integer or 32-bit float). It defaults to 48 kHz / 24-bit, is set when the project is created, and is shown and changeable in Project Settings. All media (imported, recorded, bounced into the project, returned from the external editor) is stored in that format. The engine always processes in float. Changing the rate or depth later offers to convert the existing media.
- **Bounce (review).** Offline only, with external MIDI silent, as ruled. Hardware synths are recorded by tracking them as audio, which is an explicit use case (§6.7). Real-time bounce and an external-instrument round trip are deferred.
- **MIDI input effects (review).** Included at a basic level: a per-lane chain with a built-in set (§6.8).
- **Monitoring.** Software monitoring through the lane's mixer strip (Auto / Input / Off), plus MIDI thru.
- **Automation recording input.** Full control surfaces: the on-screen controls, MIDI learn, Mackie Control and HUI.
- **Extras included.** The media pool with offline/relink, the external sample editor, and bounce/export. Freeze is deferred.
- **Approach B.** AudioToolbox (ExtAudioFile) decodes and converts on import only, behind a `MediaImporter` trait. CAF I/O, streaming, recording and bounce are platform-neutral Rust.
- **Plans.** One spec, executed as seven sequential plans, 5a–5g (§13).

## 3. Architecture

```
Swift  VizDAWEditors   RegionStripView/ArrangementView: waveforms, fade/gain handles · LaneHeaderView: input, monitor, meter · MediaPoolView
Swift  VizDAWAppModel  RecordModel · MediaModel · AutomationModeModel · SurfaceSettings · BounceModel · ExternalEditorWatcher
Swift  VizDAW          New Project sheet, Project Settings, record transport UI, Track Synth sheet, input effects popover, Settings › Audio/Recording/Surfaces, Bounce sheet, Media pool window
FFI    core-ffi        import · media pool · overview · record · monitor · automation gestures · surfaces · bounce
Rust   core-media      CAF read/write · MediaImporter trait · Streamer (disk thread) · RecordWriter · overviews   (new, platform-neutral)
Rust   core-surface    MIDI learn · Mackie Control · HUI protocol state machines                                   (new, platform-neutral)
Rust   core-engine     region segments in the schedule · RegionSource node · input in the renderer · record taps · offline bounce
Rust   core-model      project format · fade units · media commands · lane input/monitor/input effects · destination return latency · strip automation mode · RecordPass · learn maps
Rust   core-engine     (also) MIDI input effect chain on the real-time thread
Rust   platform-macos  hal.rs input + duplex + private aggregate · ExtAudioFile importer · CoreMIDI input routing
```

Rules carried over from sub-projects 2 and 3, binding everywhere:
- The real-time thread never allocates, locks or does file I/O. `rt-check` builds prove this for every new real-time path (region playback, the input ring, monitoring, live automation overrides).
- Every FFI call returns `Result`, and no panic crosses the boundary.
- Anything the real-time thread retires is freed on the existing retire thread.

## 4. Model changes (core-model)

### 4.1 Project audio format
- `Settings` gains `format: AudioFormat { sample_rate: u32, depth: BitDepth }`, where `BitDepth` is `Int16`, `Int24` or `Float32`.
  - Allowed rates: 44 100, 48 000, 88 200, 96 000, 176 400 and 192 000 Hz.
  - Default: **48 000 Hz, 24-bit integer**.
- **New projects.** File › New opens a small New Project sheet with the rate and depth, pre-filled with the app's "Default project format" preference (initially 48 kHz / 24-bit). The programmatic `project.new { rate?, depth? }` never shows the sheet and uses the preference for anything it leaves out.
- **Project Settings** (File › Project Settings…, a sheet) shows the format and can change it.
  - Changing the rate or depth is `SettingsCmd::SetFormat`. When the project has media, it requires converting every media item (§5.1) and is one undo step, "Change Project Format". The undo keeps the old files under the media retention rule (§6.5).
  - The sheet says how many files and how much audio will be converted. Cancelling the conversion cancels the change.
  - Lowering the depth to 16-bit dithers (TPDF). Raising it never alters samples.
- **The engine always processes 32-bit float.** The format governs only the media files and the rate the device is asked to run at (§5.6).
- **Migration of older projects** (format version bump, together with §4.2) gives a project without `format`:
  - the rate of the majority of its media items (the first item's on a tie), or 48 kHz if it has none;
  - and 24-bit.

  On open, any media item whose file is not a VizDAW CAF in the project format is offline (§4.3), and a banner offers "Convert <n> files to the project format". This is the same conversion, as one undo step. Nothing converts without the user's say-so.

### 4.2 Fades in frames
- `Fade.length` changes from `Tick` to `length_frames: u64`. Fades belong to the audio, so a tempo change must not change them, and region length and source offset are already in frames.
- The project format version is bumped. Migration converts existing tick lengths at the tempo in force at the region's start.
- The validation rule changes to: `fade_in + fade_out <= region length`.

### 4.3 Media
- `MediaItem` gains `origin: MediaOrigin { Imported { original_name }, Recorded { pass: u32 }, Bounced, ExternalEdit { from: MediaId } }`. Only `origin` and `name` (display; defaults to the file stem) are added. `path`, `sample_rate`, `channels`, `length_frames` and `content_hash` keep their meaning, and `content_hash` is now always filled (BLAKE3 of the CAF audio data, hex; `blake3` is a new pure-Rust dependency).
- Media commands move from `RoutingCmd` to a new `MediaCmd`. `AddMedia` and `RemoveMedia` move with their existing semantics, joined by:
  - `RenameMedia { media, name }`;
  - `RelinkMedia { media, path, content_hash, length_frames, channels, sample_rate }`, which refuses a shorter file while regions use frames beyond its end;
  - `CompactMedia`, which removes every unused item.
- `RemoveMedia` without a replacement is refused while the item is in use. The UI confirms and lists the affected regions (parent §5).
- **Offline media is runtime state, not model state.** `core-ffi` keeps an `offline: HashSet<MediaId>` computed on open and on file-system events. A media item is offline when:
  - its file is missing;
  - or its file is not a readable VizDAW CAF in the project format (§4.1);
  - or its content hash differs.

  Offline regions play silence and are drawn hatched. Saving never writes this flag.

### 4.4 Regions
- `AudioCmd` gains `Normalize { events, peak_db }`. It sets `gain_db` so that the region's loudest sample (taken from its overview) reaches `peak_db`. This is non-destructive (D26e).
- Region mute is the existing per-event mute.
- Regions keep `gain_db` within −144…+24 dB.

### 4.5 Lanes
`Lane` gains:
- `input: Option<LaneInput>`:
  - `LaneInput::Audio { channels: InputChannels }`, where `InputChannels` is `Mono(u16)` or `Stereo(u16)` (the left channel's index; the pair is index and index+1).
  - `LaneInput::Midi { port: Option<String>, channel: Option<u8> }`: `None` means all ports or all channels.
- `monitor: MonitorMode { Auto, Input, Off }`, default Auto.
- `input_fx: Vector<InputFx>`: the MIDI input effect chain (§6.8), at most 8 slots, empty by default.

New `LaneCmd` variants set all three. These are undoable like the other lane flags. A lane records audio only if its input is `Audio` and MIDI only if `Midi`. A lane may hold both kinds of content, but it records one kind at a time.

`LaneInput::Audio` also gains `return_of: Option<DestId>`. When set, the lane records the audio return of that external MIDI destination (§6.7).

### 4.5a Destinations
- An external MIDI destination (an instrument-list entry for a CoreMIDI endpoint) gains `return_latency_ms: f32`, default 0, range 0–500.
- This is the hardware's own delay from receiving a MIDI note to sound at its audio output: the MIDI interface plus the synth's response.
- It is set by hand or measured (§6.7). Like other destination edits, it is undoable.

### 4.6 Strip automation mode
- `Strip` gains `automation: AutomationMode { Off, Read, Touch, Latch, Write, Trim }`, default `Read`.
- It is set with `StripsCmd::SetAutomationMode`, which is undoable, like other strip settings.
- `Off` plays the static values and ignores automation. `Read` plays automation.

### 4.7 Recording pass
- `Command::Record(RecordPass)` applies a finished pass atomically and is built by `core-ffi` at Stop (§6.5). It holds:
  - new media items;
  - for each lane: events to insert, ranges to clear (replace mode), and takes to add, with the active take to select;
  - automation writes per target (§7.4).
- History gains `begin_group(label)` / `end_group()`, which merge every step made between them into one step. A recording pass opens a group at record start and closes it after the `RecordPass` at Stop. Edits made while recording are therefore undoable individually until Stop, and afterwards the whole pass, including them, is one step "Undo Record" (parent §5).

### 4.8 MIDI learn
- `Document` gains `learn: Vec<LearnMap>` for mappings whose targets are project objects. `LearnMap { source: LearnSource, target: AutomationTarget, mode: LearnMode }`:
  - `LearnSource` is a port name plus a CC7, CC14 pair, pitch bend or note, each with its channel.
  - `LearnMode` is `Absolute`, `Relative2sComplement` or `RelativeBinOffset` (the latter two are for encoders).
- Mappings to commands (registry IDs) are machine-specific and live in app preferences (§8.3).

## 5. Audio playback (core-media, core-engine)

### 5.1 Import
- **`trait MediaImporter`** lives in core-media:
  - `fn probe(&self, path) -> Result<SourceInfo>` returns the format name, rate, depth, channels, frames and duration.
  - `fn convert(&self, path, out: &Path, format: AudioFormat, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Result<ConvertedInfo>`.
- **The macOS importer** is `platform_macos::media::ExtAudioImporter`.
  - ExtAudioFile decodes to float32 at the target rate with the highest-quality converter. It accepts every format the system reads (WAV, AIFF, CAF, FLAC, MP3, AAC, ALAC…).
  - core-media's CAF writer then stores the float data in the project depth, dithering (TPDF) only when that depth is 16-bit.
- **Channels.** Mono and stereo files keep their layout. A file with more than two channels is split into mono files, one media item per channel. Dropping one onto the arrangement makes one lane per channel.
- **Where files go.** The output is `Media/<stem>.caf`, with a numeric suffix if the name is taken. The conversion writes `Media/.incoming/<uuid>.caf` and renames it into place when complete. An interrupted import leaves nothing in `Media/`.
- **Threading.** Import runs on a background thread with progress and cancellation. `Session.import_audio(paths, at: Option<(seq, lane, tick)>)` returns a job ID. The finished job applies one undo step that adds the media and, with `at`, inserts regions.
- **Changing the project format** runs the same converter CAF→CAF over every item (into new files, since the old ones stay for undo). A rate change also scales each region's `source_offset`, `length` and fade lengths by the rate ratio, rounding to the nearest frame. A depth-only change leaves frame positions alone.

### 5.2 CAF reader and writer
- These are platform-neutral Rust in `core_media::caf`.
- **Writer:**
  - It writes `desc` (linear PCM, little-endian, interleaved; float32, or int16/int24 packed as the project depth requires), `chan` and `data`.
  - It takes float input. It converts to integer with clipping at full scale, counting clipped samples, plus TPDF dither for 16-bit. 24-bit is rounded without dither, because a 24-bit noise floor is below any converter's.
  - While recording, the `data` chunk size is −1, which CAF defines as "to end of file", and an `info` chunk notes `vizdaw-recording`. On a clean close, the real size is written.
  - A crash therefore leaves a valid, readable file.
- **Reader:**
  - It accepts only what the writer produces: little-endian linear PCM as int16, int24 or float32, 1–2 channels, and a `data` size that is either known or −1.
  - It converts to float32 on the disk thread when filling stream voices and the RAM cache. The real-time thread only ever sees float.
  - It refuses everything else with a typed error. Other formats go through the importer.
  - Reads are positioned (`pread`) and never memory-mapped. A file truncated or replaced underneath playback must yield errors, not SIGBUS.

### 5.3 Schedule: region segments
- `schedule::compile` stops skipping `EventBody::AudioRegion`. For each lane of the playing structure (with references resolved under the nested timing rule), it produces non-overlapping `Segment`s by:
  - mapping region start ticks to frames;
  - applying the parent overlap rule (the later start takes over, and an earlier, longer region resumes);
  - repeating regions for looped lanes and looped references;
  - cutting at lane length.
- Each segment holds: `start_frame`, `media`, `file_frame` (the source position at `start_frame`), `frames`, `gain` (linear), and fade-in/out curves with frame positions relative to the *region*. A fade therefore stays correct when the overlap rule cuts a segment.
- Mute, solo and lane/event mute apply as they do for notes. A muted region produces no segment. Reference transpose does not affect audio.
- **Routing.** A segment goes to the `StripKind::AudioPath` strip of its lane's `audio_out`. Inserting the first region on a lane with no `audio_out` assigns the document's first output path, creating "Audio 1" if there are none, as part of the same command.
- Segments are sorted by start and shared with the streamer as `Arc<[Segment]>` per compiled schedule.

### 5.4 RegionSource node
- `NodeSpec` gains `Regions { path: OutputPathId }`: one node per AudioPath strip that has segments. It feeds that strip through an `InputSpec` like an instrument.
- In each block the node:
  - finds the segments that overlap the step;
  - reads their frames from the streamer voices (§5.5);
  - applies gain × fade curve per frame;
  - sums the result into its stereo output. Mono media goes to both channels at unity, and the strip's pan law handles placement.
- Fade curves are evaluated with closed forms (linear; equal-power `sin`; exponential with a 60 dB range; S-curve as a smoothstep of equal-power). There are no tables to allocate.
- **Delay compensation:** the node has no latency of its own and is compensated by the existing PDC, like instrument nodes.

### 5.5 Streamer (disk thread)
- **Two-layer buffering.**
  - Files of at most **4 MiB** of audio data (about 11 s of stereo at 48 kHz) are read whole, converted to float, into a shared **RAM cache** (`Arc<[f32]>`, LRU, **512 MiB** budget of float data, a Setting). The real-time thread reads them directly.
  - Larger files are streamed.
- **Voices.**
  - A fixed pool of **256 stream voices** is allocated when the engine starts. Each voice is a single-producer/single-consumer ring of **2 s** at the project rate per channel, read by the real-time thread and written by the disk thread.
  - The disk thread plans which segments need voices: those inside the next **3 s** of playback, or the loop seam (below). It assigns free voices and publishes `segment index → voice` in a lock-free slot table that the real-time thread reads.
  - It refills any voice that is less than half full, nearest-deadline first, with read sizes of at least **64 KiB**.
- **Locate.**
  - On a locate (or play from a new position), the disk thread fills the voices for the new position.
  - The transport waits up to **250 ms** for those voices to be ready before rolling. After that it rolls anyway, and anything not ready plays silence. That is reported as a late start, not as an underrun.
- **Loop.** While looping, the first **2 s** after the loop start is kept prefilled in a second voice for every segment there, so a wrap never waits on the disk.
- **Underrun.** If a voice lacks the frames the real-time thread needs, that segment plays silence for those frames. An atomic `stream_underruns` counter increments, and the UI shows a non-modal "Disk too slow" message, at most once per playback. Underruns never stop the transport.
- **Offline media** gets no voice and plays silence. A file that errors mid-read becomes offline (§4.3) and is reported once.
- **Bounce** uses a synchronous mode in which reads block (§9).

### 5.6 Device rate
- When the project rate differs from the device rate (on open, on New, or on a device change), the engine asks the HAL to switch the device's nominal rate.
- If the device refuses:
  - MIDI and plugins play at the device rate;
  - audio regions are muted;
  - a persistent banner says so and offers "Change project rate to <device rate>", which is the §4.1 conversion.
- There is no real-time resampling.

## 6. Recording

### 6.1 Devices (platform-macos hal.rs)
- `AudioConfig` gains `input_device: Option<String>` (`None` = same device as output) and `input_enabled: bool`.
- `StreamInfo` gains:
  - `input_channels: u32` and `input_channel_names: Vec<String>`;
  - `input_latency_frames` and `output_latency_frames`, each being device latency + stream latency + safety offset, as the HAL reports per direction.
- **Same device:** one duplex IOProc. **Different input device:** VizDAW creates a *private* aggregate device (`kAudioAggregateDevicePrivateKey = 1`) with the output device as clock master and drift correction on the input. It is destroyed on close. This keeps parent §6's promise of separate devices.
- Input is opened only while some lane is armed or monitored, or the input meters are shown. Otherwise the output-only path runs as today (no microphone permission prompt until needed).
- **Microphone permission:**
  - The first input open triggers macOS's prompt; the app's Info.plist carries `NSMicrophoneUsageDescription`.
  - If permission is denied, arming an audio lane shows a message pointing to System Settings › Privacy.
- `Renderer::render` gains an input argument: planar `&[&[f32]]` for this block, empty when input is closed.

### 6.2 Input ring and writer
- **In the real-time thread.** The renderer copies the input channels that armed lanes use into one **input ring**: interleaved by used channel, **4 s** capacity, single producer and single consumer. Each block is stamped with its timeline frame and with whether recording is active.
- **The RecordWriter thread** (core-media) drains the ring and writes one CAF per armed audio lane, in the project format (§4.1) and mono or stereo per `LaneInput`, into `Media/Recorded/<lane name> <yyyy-mm-dd HHmmss>.caf`. It flushes the CAF header every 2 s.
- **Clipping.** Clipped samples in integer depths are counted per lane and shown as a clip mark on the lane's input meter. A float project never clips on disk.
- **Ring overflow** (the writer too slow):
  - the lost frames are written as silence, so the timing stays right;
  - `record_overflows` increments;
  - the pass is flagged so that its undo label reads "Record (with gaps)", and a message is shown.
- **Disk-space check.** Before recording starts, and every 10 s while it runs, free space on the project volume is checked. Below **1 GiB**, a warning is shown. Below **100 MiB**, recording stops cleanly (as if Stop had been pressed).
- **Overviews** (§10.1) are built while the file is written, so the region draws as it grows.

### 6.3 Latency compensation
- **Audio.** A captured input frame is placed at the timeline frame the performer was hearing when they played it:
  - Per IOProc call, let `in_time` and `out_time` be the sample times of the input and output buffers' first frames (from the IOProc time stamps) and `T` the timeline frame of the output buffer's first frame.
  - The input buffer's first frame lands at `T − (out_time − in_time) − output_latency − input_latency + user_trim`.
  - The latencies come from §6.1, as reported by the HAL.
  - `user_trim` is a ±frames Setting (Settings › Recording › Latency trim, default 0) for devices that misreport.
  - The writer records from the first stamped block, and the region's `source_offset` absorbs the shift. No audio is discarded.
- **MIDI.** An incoming event is placed at the timeline position that was audible at its host time:
  - convert the host time to a frame through the clock's host-time mapping;
  - subtract the output latency.
- **External MIDI output** is scheduled by host time (sub-project 2). 5c makes sure that time is when the event's frame is *heard*, adding the output latency if the existing scheduler does not, and pins this with a test. This shared audible-time clock is what lets §6.7 line up a synth's audio with the MIDI that played it.
- **Calibration.** The manual checklist includes a loopback calibration: route output to input, record a click, and expect alignment within ±2 frames. A fake device does the same in CI (§12.1).

### 6.4 Record modes and transport
- **Arming.** The engine records only lanes that are armed (`Lane.arm`, existing) in the sequence currently playing at top level. Lanes inside referenced sequences cannot record.
- **`RecordSpec`** is passed by `Session.record(spec)`:
  - `replace: bool` (false = overdub);
  - `punch: Option<(Tick, Tick)>` (from `TransportMarks` when punch is on);
  - `loop_range: Option<(Tick, Tick)>` (the transport loop when loop is on);
  - `count_in_bars: u8` (0–4) and `pre_roll_bars: u8` (0–8);
  - `passes_as_takes: bool` (default true).
- **Count-in and pre-roll:**
  - Count-in plays the click before the start point without moving the timeline.
  - Pre-roll starts playback that many bars early.
  - Audio is captured from the moment the transport rolls, so pre-roll audio is kept as region handles. Regions and MIDI events cover only the record range.
- **Record range.**
  - With punch on, it is the punch range.
  - With loop on, it is the loop range (and punch, if also on, narrows it within each pass).
  - Otherwise it runs from where recording started until Stop.
- **Replace and overdub.**
  - *MIDI.* Replace removes the lane's existing MIDI events (in the active take) whose start lies in the recorded range, and only in that range (parent §6). Overdub merges.
  - *Audio.* One lane plays one region at a time (the overlap rule), so audio overdub behaves like replace for the overlapped range. To layer audio, record into another lane or take. The record popover says this when overdub is on and an audio lane is armed.
- **Loop recording.**
  - Each wrap ends a pass. Audio uses one continuous file per lane for the whole run; each pass is a region over its slice of that file.
  - `record.keep_pass` keeps all passes so far. `record.discard_pass` drops every pass since the last keep. These commands are already registered in 4f; 5c gives them handlers.
  - At Stop:
    - With `passes_as_takes`, each kept pass, plus the last pass if not discarded, becomes a new take named "Take n", and the last becomes the active take.
    - Without it, MIDI passes merge into the active take (overdub). Audio passes still become takes, since they cannot merge.
- **Step recording** is the 4f step entry. Starting Record while step entry is on is refused with a message.

### 6.5 Finishing a pass
At Stop (or at the auto-stop in §6.2):
1. The writer closes the files, fixing the CAF sizes.
2. `core-ffi` builds the `RecordPass` (§4.7) from:
   - the collected MIDI;
   - the region spans;
   - the passes and their keep/discard state;
   - the automation writes.
3. It applies the `RecordPass` and closes the history group.

**Recorded media and undo:**
- A media file stays on disk while the document *or any undo/redo step* refers to it.
- On save and on close, files under `Media/` referenced by neither are moved to the macOS Trash. They are never unlinked, so a mistake is recoverable.
- This one rule also covers compact (§10.2) and external edits.

**Crash recovery:**
- On open, `Media/Recorded/` files that no media item references and whose `info` chunk says `vizdaw-recording` are offered in the existing recovery sheet ("Recovered recordings: import into a new lane / move to Trash").

### 6.6 Monitoring and MIDI thru
- **Audio monitoring.** An armed or `Input`-monitored lane's input feeds its AudioPath strip through the `RegionSource` node's input mix (the node exists for every AudioPath strip that has segments or a monitored input), with no extra latency beyond the device's round trip. When an input is monitored, the strip's inserts process it like playback.
- **The modes:**
  - **Auto**: the input is heard when the lane is armed and either the transport is stopped, or it is recording and the play position is inside the record range. Outside the range, playback is heard. This is auto-input.
  - **Input**: always the input.
  - **Off**: never.
- **MIDI thru.** Incoming MIDI matching an armed lane's `LaneInput::Midi` is sent at once to that lane's destination, through the existing live note path that step entry and audition use.
- **Input effects.** Thru and record both pass through the lane's MIDI input effect chain (§6.8).

### 6.7 Tracking a hardware synth as audio
This is an explicit use case: printing an external synth into the project as audio.

- **Setup.**
  - A MIDI lane plays to an external destination D, the synth.
  - An audio lane has `LaneInput::Audio { channels, return_of: Some(D) }`, the interface inputs where the synth's outputs are wired.
  - The **Track Synth…** command on a MIDI lane whose destination is external does this setup in one undo step:
    - It creates or reuses that audio lane, named "<D name> audio", just below.
    - It sets the lane's input, using a channel picker in the sheet.
    - It arms the audio lane.
    - It leaves the MIDI lane playing. Optionally, the MIDI lane is armed too, to record the keyboard performance at the same time.
- **One pass.** In a single Record pass:
  - the MIDI lane's existing events (or the live performance, if it is armed) play to the synth;
  - the audio lane records the return.
  
  Both kinds of recording share the pass, the history group and the one "Undo Record" step (§6.5). A loop pass makes matching MIDI and audio takes.
- **Alignment.**
  - Ordinary latency compensation (§6.3) places the return audio at the frame that was audible when it arrived.
  - External MIDI leaves at audible time (§6.3).
  - For a `return_of` lane, the recorded region is additionally shifted earlier by D's `return_latency_ms` (§4.5a).
  
  So the audio lines up with the MIDI notes that played it. The residual target is within ±1 ms once the latency is measured.
- **Measure.** The Track Synth sheet and the destination editor have a **Measure** button:
  - It sends 5 short notes (pitch 60, velocity 100, 500 ms apart) to D with the transport stopped.
  - It detects each onset on the return input: the first sample above −40 dBFS after the note.
  - It sets `return_latency_ms` to the median delay, minus the round-trip latency the HAL reports.
  - It fails with a message ("No signal on the synth's return input") if fewer than 3 onsets are found within 500 ms of their notes.
- **Monitoring.** The return lane monitors in Auto by default, so the synth is heard through its strip while tracking. Its MIDI lane's own instrument strip is unaffected, because external destinations have none.
- **After tracking.** The sheet offers "Mute the MIDI lane after recording" (off by default). If chosen, the mute is part of the pass's undo step. The MIDI is kept either way, so the part can be re-tracked after edits.

### 6.8 MIDI input effects (D26h)
- **Model.** `Lane.input_fx` (§4.5) is an ordered chain of up to 8 `InputFx { kind: InputFxKind, bypass: bool }`.
- **The built-in set** (`InputFxKind`):
  - **Arpeggiator**:
    - mode Up, Down, Up-Down, Down-Up, As Played, Random or Chord;
    - rate as a note value from 1/1 to 1/64, straight, dotted or triplet;
    - octaves 1–4;
    - gate 5–200 %;
    - velocity As Played or Fixed (1–127);
    - Latch (held notes keep playing after release until new notes arrive);
    - Sync: to transport bars and beats while playing, and free-running at the project's current tempo when stopped.
  - **Transpose**: ±48 semitones.
  - **Velocity**: scale (0–400 %), offset (±127), clamp minimum and maximum, or Fixed.
  - **Chord**: adds up to 6 notes at given semitone intervals (±48) to each note.
  - **Input Quantize**: grid (the shared snap values) and strength (0–100 %). It moves note starts *only in what is recorded*. Live thru is never delayed, so quantizing does not add latency to what you hear.
- **Where the chain runs.**
  - The chain runs on the real-time thread, inside the renderer, per armed or `Input`-monitored MIDI lane, so the arpeggiator is sample-accurate and synced.
  - Incoming events arrive from CoreMIDI through a lock-free queue, already timestamped (§6.3).
  - Each effect has fixed-capacity state: up to 32 held notes and 128 sounding notes. Overflow drops the newest note and counts it.
  - Chain changes are published like other graph updates. A changed or bypassed effect first sends note-offs for every note it is sounding, so no note hangs.
- **Recording is post-effect.** Main spec §6: "what is recorded is the stage's output".
  - The recorder receives exactly what thru sent to the destination, with Input Quantize then applied to note starts.
  - The raw input is not kept.
  - To record the raw performance, bypass the chain.
- **Lanes without an armed or monitored state** do not run their chain. Playback of recorded events never passes through input effects.
- **UI.** An "FX" button on the lane header (lit when the chain is non-empty and not all bypassed) opens a popover:
  - the chain list: add, remove, drag to reorder, bypass;
  - the selected effect's parameters.

  Settings are undoable lane edits (one step per gesture).

### 6.9 Record UI
- **Transport toolbar:**
  - a Record button (R, `transport.record`), shown red while recording;
  - a Record popover with Replace/Overdub, Punch (shows and edits the 4f punch fields), Loop, Count-in, Pre-roll and Passes as takes;
  - a record-time remaining readout, based on disk space and the number of armed channels;
  - pass and keep counters during a loop record.
- **Lane menu:** Track Synth… (§6.7) on MIDI lanes whose destination is external.
- **Lane header (`LaneHeaderView`)** keeps its existing R toggle and adds:
  - an input picker (audio channels by the device's channel names, or a MIDI port and channel);
  - a monitor button that cycles A / I / –;
  - a thin input meter while the lane is armed or monitoring, with a clip mark;
  - the FX button (§6.8);
  - for a `return_of` lane, a small link badge naming the synth.
- **Mixer (`MixerView`).** AudioPath strips show the same input picker and monitor button above the inserts.
- **Settings › Audio:**
  - input device ("Same as output" or a device);
  - latency readout and trim;
  - RAM cache budget;
  - the streaming setting "Disk read-ahead" (2–8 s, default 3 s; ring size follows).
- **Project Settings** (§4.1): sample rate and bit depth, with conversion.
- **Settings › Recording:**
  - default project format for new projects (48 kHz / 24-bit);
  - default count-in and pre-roll;
  - passes as takes;
  - default monitor mode for new lanes;
  - low-disk thresholds (shown only).

## 7. Automation recording

### 7.1 Gesture channel
Every user-driven control change becomes a **gesture**:
- `touch(target)`;
- one or more `value(target, v)`;
- `release(target)`.

The gesture's target is an `AutomationTarget` (4d) with a normalised value.

Where gestures come from:
- **On-screen mixer** (`MixerView`): mouse down, drag and mouse up. Click-to-toggle controls (mute) send touch, value and release at once.
- **Plugin windows:**
  - *AU*: an `AUParameterTree` observer, plus `kAudioUnitEvent_Begin/EndParameterChangeGesture` from AU v2 event listeners.
  - *CLAP*: `CLAP_EVENT_PARAM_GESTURE_BEGIN/END` plus `CLAP_EVENT_PARAM_VALUE` from the plugin's output events.
  - The host forwards these into the same channel. A plugin that sends values without gesture events is treated as touched by its first value and released **500 ms** after its last.
- **MIDI learn and HUI/MCU faders without touch sensing:** the same 500 ms release-after-idle rule.
- **Touch-sensitive surfaces:** real touch and release messages (§8).

The FFI is `Session.gesture(target, phase: Touch|Value(f64)|Release)`. It replaces direct `set_gain`/`set_pan`/`set_param` calls for user moves. Those calls remain for non-gesture edits such as typed values.

### 7.2 What a gesture does
- **While a target is touched**, its live value is applied at once as the engine override (the existing `graph::controls` override mechanism), so it is heard immediately and wins over automation playback for that target.
- **When the strip's mode is Off or Read**, a gesture outside recording changes the static value. This is one undo step per gesture (like today's `set_gain` + `begin_edit`/`end_edit`).
  - In Read, while playing, an automated target snaps back to its automation on release. Its static value still changes, but that is only heard where there is no automation.

### 7.3 Writing modes
Writing happens only while the transport is playing. Recording audio or MIDI is not needed. Each mode decides when its strip writes:

| Mode | Writes |
|---|---|
| **Write** | From play start to Stop (or the next loop wrap), every target of the strip: fader, pan, mute and sends, at their current values, touched or not. Plugin parameters of the strip's inserts are written only once touched (writing every parameter of every plugin would bury the lanes). After the pass, the strip's mode becomes the Settings › Automation "After Write" mode (default Latch). |
| **Touch** | From touch to release. On release, the target returns to its existing automation over the **return time** (Setting, 0–2000 ms, default 250 ms). The glide is written as a straight ramp. |
| **Latch** | From touch until Stop or the next loop wrap, holding the last value after release. |
| **Trim** | Like Touch in timing. Instead of absolute values it writes a **relative offset**: the control's movement since touch, measured in dB for fader and send targets and in normalised units for pan and plugin parameters, is *added* to the existing automation under it, so the existing shape is kept. Where the target has no automation, the offset applies to the static value, writing absolute points. Stepped targets (mute, solo, stepped plugin params) treat Trim as Touch. |

Rules common to all modes:
- **Loop wrap** ends the writing of every target. The next cycle starts again in the strip's mode, and for Write that is now the "After Write" mode. This keeps a Latch or Write pass from overwriting the start of the loop with the values held at its end.
- **Capture.** Values are sampled on the control thread every **10 ms** while they change, and once at touch and release. Each sample is stamped with the transport frame derived from host time, the same mapping as MIDI (§6.3).
- **Thinning.** When a write ends, the samples are thinned with Ramer–Douglas–Peucker:
  - tolerance **0.1 dB** for fader and send targets (in the dB domain);
  - **0.25 %** of range for others (Setting: Low / Medium / High);
  - stepped targets keep only their changes.
- **Replacing.** The written range replaces existing automation for that target in `[first sample, last sample]`. A point is added at each edge so that values outside the range are unchanged. Trim modifies existing points in place, plus the edge points.

### 7.4 Where points go
- Automation events live in ordinary lanes (4d). A write for target T goes into the first lane of the top-level playing sequence that already holds automation for T.
- If there is none, a new lane named "<strip> <parameter>" is created at the end of that sequence, with its Strip Chart view set to T.
- Writes are collected while playing and applied:
  - as part of the `RecordPass` when audio or MIDI is also recording;
  - otherwise as their own `Command::Record` with label "Write Automation", one undo step per pass, at Stop.
- The Strip Chart draws the in-progress write live from `Session.automation_preview(target)`.

### 7.5 UI
- **Mixer:** each strip gets an automation-mode popup (Off / Read / Touch / Latch / Write / Trim), each mode with its own colour, and touched controls get a ring.
- **Strip menu:** a "Set all strips to…" command.
- **Settings › Automation:** After Write mode, return time, thinning, and "Write plugin parameters only when touched" (always on, shown for clarity).
- **Commands:** `automation.mode.<mode>` for the selected strips, plus `automation.suspend` (a global toggle that makes every strip behave as Read without changing its mode).

## 8. Control surfaces (core-surface)

### 8.1 Engine
- **The new crate `core-surface`** is pure Rust with no I/O. A `Surface` is a state machine:
  - `fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction>`
  - `fn feedback(&mut self, state: &MixerView) -> Vec<Vec<u8>>`, the messages to send.
- **`SurfaceAction`:**
  - `Gesture(target, phase)` (into §7.1);
  - `Command(registry ID)` (transport, locators, undo…);
  - `Bank(±n)`, `Channel(±1)`;
  - `Select(strip)`;
  - `Jog(delta)`, which nudges the position.
- **The MixerView snapshot** gives the surface what it displays: strip names, fader, pan, mute, solo, arm, select, automation mode, meters, and the position as bars·beats or timecode.
- **The app side** (`core-ffi::surfaces`) owns the CoreMIDI ports through `platform-macos::midi`:
  - incoming bytes go to `Surface::input`;
  - actions are applied;
  - `feedback` is sent at most every 20 ms, or at once for touched faders' partners (motor faders must not fight a hand: no fader feedback while touched).

### 8.2 Protocols
- **Mackie Control (MCU):**
  - The main unit plus up to 3 extenders (XT), giving banks of 8 × units.
  - Faders are 14-bit pitch bend per channel with touch notes (0x68–0x70).
  - V-pots are relative CC 0x10–0x17 in pan, send or plugin assignment mode, with LED-ring feedback.
  - Mute, solo, rec-arm and select buttons; transport and bank/channel buttons; jog wheel.
  - Scribble-strip LCD via SysEx (names and values, 2 × 56 characters per unit); 2-digit assignment and timecode/BBT displays.
  - Meters via channel pressure.
  - The device-query handshake is answered.
- **HUI:**
  - Zones and ports via CC 0x0F/0x2F select-plus-port messages.
  - Faders as hi/lo CC pairs with touch via zone 0x0..0x7 port 0.
  - V-pots, mute, solo, rec and select per channel; transport.
  - 4-character scribble names and the 40-character display via SysEx.
  - Meters via polyphonic pressure (0xA0).
  - The 1 s ping is answered.
- **MIDI learn.**
  - *Learning.* A "Learn" button on any automatable control (mixer, plugin window header, Strip Chart target menu) waits for the next incoming CC, pitch bend or note, and creates a `LearnMap` (§4.8).
  - *Command mappings.* These are made in Settings › Keys next to the key recorder ("Learn MIDI").
  - *Pairs and encoders.* 14-bit pairs are detected when CC n and n+32 arrive together. Relative encoders are detected when values cluster around 1/127 or 63/65, and can be overridden in the mapping list.
  - *Feedback.* Absolute mappings send the value back so that motorised and LED-ring controllers follow.

### 8.3 Devices and persistence
- **Settings › Surfaces** lists surfaces: type (MCU, MCU XT, HUI), input port, output port, and the unit's position in the bank.
- They are stored in app preferences (`~/Library/Application Support/VizDAW/surfaces.json`, beside the 4f key bindings), because hardware belongs to the machine.
- Command learn maps go in the same file. Target learn maps are in the project (§4.8).
- A missing port leaves the surface listed as "disconnected", and it reconnects when the port appears (CoreMIDI notifications).

## 9. Bounce and export

### 9.1 Render
- **The offline backend** (`core_engine::offline`) drives a copy of the engine's renderer on a worker thread, faster than real time, with the streamer in synchronous mode. Plugins are switched to offline rendering:
  - AU: `kAudioUnitProperty_OfflineRender`;
  - CLAP: the `render` extension's offline mode, when the plugin offers it.
- **The source** is one of:
  - **Mix**: master;
  - **Stems**: each chosen strip, post-fader, one file each;
  - **Selection**: master, limited to the arrangement's time selection.
- **The range** is the song (start to end) or the time selection, plus a **tail**: render on until the output stays below −96 dBFS for 0.5 s, capped at 10 s (Setting).
- **External MIDI destinations** cannot be rendered offline. If the bounce range plays to any, the sheet warns before rendering ("<n> lanes play external MIDI and will be silent. Track them as audio first: Track Synth…"). Real-time bounce and an external-instrument round trip are deferred (§1).
- **Live playback** keeps running on the device while a bounce renders. The two never share a renderer, and plugin instances are separate copies restored from the saved state.

### 9.2 Format and destination
- **Format:**
  - WAV, AIFF or CAF;
  - 16- or 24-bit integer with TPDF dither (on by default for 16-bit), or 32-bit float;
  - any rate the converter supports.
  - The sheet defaults to the project's rate and depth (§4.1), as WAV.
  - Files are written through `MediaImporter`'s converter on macOS (`ExtAudioFile` with a float client format and the file format as chosen).
- **Destination:**
  - a chosen file (the save panel). Programmatic bounce never opens dialogs (§12.1);
  - and/or "Add to project", which stores the result in the project format (§4.1) as a media item (origin `Bounced`) and places it as a region on a new lane at the range start. This is one undo step.
- **Progress** shows in a sheet, and the bounce can be cancelled. A cancelled or failed bounce deletes its partial file.

## 10. Editors and media pool (Swift)

### 10.1 Waveform overviews
- core-media builds a multi-resolution min/max peak file per media item: levels of 256, 4096 and 65 536 frames per peak, per channel, i16 min/max.
- The file is `Media/.overviews/<content hash>.vzov`, rebuilt in the background if missing or stale. It is built during import, recording and bounce.
- `Session.overview(media, from_frame, to_frame, columns) -> OverviewRows` picks the right level and returns per-channel (min, max) per column. Below 256 frames per column it reads samples directly through the RAM cache or the file.
- Regions draw their waveform scaled by `gain_db` and shaped by their fades in:
  - `RegionStripView` (piano roll);
  - the arrangement blocks.

### 10.2 Region editing
- **Handles in the arrangement and the region strip:**
  - The top-left and top-right corner handles drag the fade lengths.
  - A context menu on a fade sets its shape.
  - The top-edge line drags the gain (±dB readout). ⌥-click resets it to 0 dB.
  - Each drag is one transaction (the existing gesture machinery).
- **Menu commands** for the selected regions: Normalize… (peak dB), Fade In to Cursor / Fade Out from Cursor, Remove Fades.
- **Dropping audio files** onto the arrangement or region strip imports them (§5.1) at the drop tick. Dropping onto empty space below the lanes creates new lanes.

### 10.3 Media pool window
- **Rows:** name, duration, channels, origin, use count, used percentage, and an offline mark.
- **Actions:** select regions using an item; Select Unused; Compact (removes unused items, one undo step, with the files trashed per §6.5); Rename; Relink…; Reveal in Finder; Import….
- **Deleting** an item that is in use asks for confirmation and lists the regions (parent §5).
- **Relink dialog:**
  - On open, offline items are listed with Locate… (a file panel) and "Search folder…", which matches by content hash, then name and length.
  - A chosen file that is not a VizDAW CAF is imported (converted) and relinked to the converted copy.
  - Each relink is one undo step.
  - "Leave offline" closes the dialog.

## 11. External sample editor
- **Settings › Editing** gains "External sample editor" (an application chooser).
- **"Open in External Editor"** on one region:
  - writes the region's span (`source_offset`, `length`, with no gain or fades) as WAV at the project rate and depth to `Media/Edits/<name> edit <n>.wav`, because external editors read WAV more widely than CAF;
  - opens it with the chosen app;
  - watches it with a `DispatchSource` file-system-object watch for write, rename or delete events.
- **Re-importing on change:**
  - A change is accepted once the file's size and modification time have stayed the same for 1 s.
  - The file is imported (converted to CAF in the project format, origin `ExternalEdit { from }`).
  - That region alone is relinked to it: `media` = new, `source_offset` = 0, `length` = the new file's length, with fades clamped. This is one undo step, "External Edit".
  - Other regions using the original are unaffected.
- **Watching** lasts until the project closes. "Reload External Edit" on a region whose media came from an external edit re-checks the `.wav` by hand later.
- **The `.wav` copies** follow the media retention rule (§6.5) once nothing refers to them.

## 12. Control channel, errors, testing

### 12.1 Control-channel additions
- **Commands.** These follow the rule "programmatic calls never open dialogs"; file paths come as parameters.
  - **Record:** `transport.record` (handler added), `record.keep_pass`, `record.discard_pass`, `record.set { replace, punch, loop, count_in, pre_roll, passes_as_takes }`.
  - **Project:** `project.new { rate?, depth? }`, `project.set_format { rate, depth }` (converts, waits for completion).
  - **Lanes:** `lane.set_input` (including `return_of`), `lane.set_monitor`, `lane.set_input_fx { lane, chain }`, `lane.track_synth { lane, channels, arm_midi?, mute_after? }`.
  - **Destinations:** `destination.set_return_latency`, `destination.measure_return { dest }`, which returns the measured milliseconds.
  - **Media:** `media.import { paths, seq?, lane?, tick? }`, `media.relink { media, path }`, `media.compact`, `media.rename`.
  - **Regions:** `region.normalize`, `region.open_external` (on CI it uses a configured stub editor command).
  - **Automation:** `automation.set_mode { strips, mode }`, `automation.gesture { target, phase, value? }`, `automation.suspend`.
  - **Surfaces and MIDI:** `surface.add`, `surface.remove`, `surface.inject { surface, bytes }`, `midi.inject { port, bytes }`.
  - **Bounce:** `bounce { source, range, format, bits, rate, dither, path, add_to_project }`, which waits for completion and returns the file info.
- **`state.get` gains:**
  - `record`: recording, spec, current pass, kept passes, underruns, overflows, late starts;
  - `media`: per item its ID, name, path, offline flag, use count and origin;
  - `automation`: mode per strip, touched targets and suspend;
  - `surfaces`: each surface's connection plus the last feedback bytes per surface (for tests);
  - `input`: device, channels, latency, meters and clip counts;
  - `project`: format (rate and depth), and media not in the project format;
  - per lane: `input_fx` (the chain) and `input.return_of`.
- **Fake devices for e2e.** `VIZDAW_AUDIO=offline` (existing) gains a fake input, `VIZDAW_AUDIO_INPUT=fake`:
  - channel 1 carries a 440 Hz sine at −12 dBFS;
  - channel 2 carries an impulse every 0.5 s;
  - `VIZDAW_AUDIO_LOOPBACK=<frames>` instead feeds the output back into the input after that many frames. This drives the latency test.
  - `VIZDAW_FAKE_SYNTH=<delay ms>` adds a fake external MIDI destination, "Fake Synth". Every note-on sent to it renders a 5 ms click on fake input channel 3 that many milliseconds after the note's audible time, and note-off ends a 220 Hz tone started with it. This drives the synth-tracking tests.
- **Fake MIDI and surfaces.** `midi.inject` feeds bytes as if from a named port, and `surface.inject` feeds a surface directly, so no CoreMIDI virtual ports are needed on CI.

### 12.2 Errors and faults
- **Real-time fault flags** gain `stream_underrun`, `late_start`, `record_overflow` and `input_lost` (the input device disappeared mid-pass, which stops the pass cleanly and keeps what was recorded).
- **Import errors** (unreadable or unsupported files) are reported per file. The others still import.
- **A device that loses input while recording** ends the pass as if Stop had been pressed, and reports this.
- **Missing permissions** (microphone) and full disks give clear non-modal messages. They never panic and never leave a partial project state: the `RecordPass` either applies whole or not at all.
- **Surfaces:** a malformed or unknown message is ignored and counted. A surface never stops the app.

### 12.3 Testing
- **core-media:**
  - CAF write→read round trip (mono and stereo, sizes −1 and known);
  - truncated-file reads;
  - reader refusal of other formats;
  - CAF in each depth (int16, int24, float32): round trip within one quantization step, dither present only at 16-bit, clipping counted;
  - importer conversion on macOS into each project format (WAV, AIFF and FLAC fixtures generated in tests, rate change 44.1→48 with length within ±1 frame);
  - project-format change: media converted, regions rescaled, undo restores the old files;
  - migration: an old project takes the majority media rate and 24-bit, and mismatched media are offline until converted;
  - overview levels against a brute-force min/max.
- **Streaming properties** (proptest), with random region layouts, overlaps, looped lanes, locates and loop wraps:
  - offline renders with the streamer equal renders from fully RAM-cached media, bit-exactly;
  - forcing a starved disk yields silence plus the underrun flag, never garbage or a panic.
- **Schedule:**
  - the overlap rule (the later start takes over, and the earlier resumes);
  - fade curves at the edges and when cut;
  - nested references with audio;
  - routing to AudioPath strips;
  - auto-assignment of `audio_out`.
- **Recording:**
  - `RecordPass` apply/undo property (apply then undo restores the document exactly);
  - replace only within the range;
  - overdub merge;
  - loop passes to takes, and keep/discard;
  - the history group merging edits made during recording;
  - loopback alignment within ±2 frames with the fake device;
  - overflow writes silence and flags;
  - crash-recovered files are found and offered;
  - recorded files are in the project format.
- **Synth tracking** (`VIZDAW_FAKE_SYNTH`):
  - one pass with an armed `return_of` lane: the clicks land on the MIDI note starts within ±1 ms, with `return_latency_ms` set to the fake delay;
  - Measure finds the fake delay within ±1 ms and fails cleanly with no signal;
  - Track Synth sets up and arms in one undo step;
  - a loop pass makes MIDI and audio takes together.
- **MIDI input effects:**
  - golden tests per arpeggiator mode, rate (including triplets) and octave count, synced and free-running;
  - Latch, transpose, velocity, chord and clamps;
  - chain order matters (transpose then chord ≠ chord then transpose where clamps apply);
  - bypass and chain changes send note-offs (no hung notes);
  - capacity overflow is counted;
  - recording equals thru output, with Input Quantize applied to recorded starts only;
  - `rt-check` covers the chain.
- **Automation:** golden tests per mode from scripted gesture sequences (touch, values and release at known frames):
  - Write fallback;
  - Touch return glide;
  - Latch until Stop and at loop wrap;
  - Trim adding to an existing ramp;
  - stepped targets;
  - thinning tolerance bounds;
  - edge points leaving outside values unchanged.
- **Surfaces:** byte-level golden tests per protocol:
  - MCU fader in/out, touch, bank shift with 1 + 3 extenders, scribble SysEx and V-pot rings;
  - HUI zone/port fader pairs, ping and display;
  - MIDI-learn detection (14-bit pairs, encoder styles);
  - no fader feedback while touched.
- **Bounce:**
  - an offline bounce equals an offline render of the same range;
  - dither changes only the least significant bits;
  - the tail stops at the threshold;
  - "Add to project" places the region correctly;
  - cancel removes the partial file.
- **`rt-check`** covers region playback, monitoring, the input ring and gesture overrides.
- **Swift:**
  - `RecordModel` and `MediaModel` with `FakeSession`;
  - handle hit-testing and drag maths for fades and gain;
  - the overview drawing geometry;
  - the external-edit watcher's stable-file rule (temp files);
  - every new registry ID has a handler.
- **e2e:**
  - record audio from the fake input and check the region and waveform;
  - loop-record 3 passes, keep 2, check the takes;
  - track the fake synth: check the audio region's clicks line up with the MIDI lane's notes;
  - `midi.inject` held chord into a lane with an arpeggiator while recording: check the recorded notes are the arpeggio;
  - `project.new` at 44.1 kHz / 16-bit, import a 48 kHz file: check the media's stored format;
  - Touch-write a fader via `automation.gesture` while playing and check the points;
  - drive a fader and a button from `surface.inject` (MCU) and check the feedback;
  - bounce and check the file.
- **Manual checklist** in `app/README.md`:
  - real interface record and monitor;
  - the loopback calibration;
  - aggregate (separate input) device;
  - plugin-window automation;
  - a real MCU or HUI unit if one is available;
  - tracking a real hardware synth with Measure, and checking alignment by ear and by zooming in;
  - the arpeggiator against a real keyboard, synced and free-running;
  - external editor round trip;
  - unplugging an interface mid-recording.

## 13. Plans

Plans run one after another by subagent-driven development under the standing authorization, with rulings in each ledger. Push and merge need the user's say-so.

| Plan | Deliverable |
|---|---|
| **5a — Audio playback** | `core-media` (CAF in all three depths, `MediaImporter`, streamer, RAM cache); ExtAudioFile importer; project format (`Settings.format`, New Project sheet, Project Settings, convert on change, migration) and device-rate handling; fades in frames with migration; `MediaCmd`; segments in the schedule; `RegionSource` node; offline media as silence; `media.import` and `state.get media`. Imported audio plays, loops, locates and underruns safely. |
| **5b — Regions in the editors** | Overviews (build, cache, FFI); waveforms in the arrangement and region strip; fade and gain handles and region menu commands (Normalize); drag-in import; media pool window; offline/relink dialog; media retention (Trash on save/close). |
| **5c — Recording engine** | HAL input (duplex and private aggregate), mic permission; input ring + `RecordWriter`; latency compensation; `LaneInput`/`MonitorMode`; MIDI timeline recording and thru through the lane's input-effect chain hook (empty until 5g); `RecordSpec` modes, punch, loop passes, keep/discard, takes; history groups and `RecordPass`; crash recovery of recordings; fake input and loopback; return latency on destinations, `return_of` alignment and Measure; fake synth; record and synth-tracking e2e. |
| **5d — Record UI** | Record button and popover, count-in and pre-roll, lane header input picker, monitor button and meter, mixer input controls, Settings › Audio/Recording, record-time readout and disk warnings; Track Synth sheet and lane menu. |
| **5e — Automation recording** | Gesture channel (mixer, AU and CLAP plugin windows); `AutomationMode` per strip and UI; Write/Touch/Latch/Trim engine with capture, thinning, lane placement, live preview; `automation.*` commands; golden tests. |
| **5f — Surfaces, bounce, external editor** | `core-surface` (MIDI learn, MCU + XT, HUI) and Settings › Surfaces; learn UI; bounce/export (offline render, stems, formats, dither, add to project); external sample editor; `surface.*`, `bounce`, `region.open_external`. |
| **5g — MIDI input effects** | `InputFx` model and commands; real-time chain in the renderer (arpeggiator, transpose, velocity, chord, input quantize) feeding thru and the recorder; FX button and popover; `lane.set_input_fx`; golden tests and e2e. Until this plan lands, the chain is empty and thru/record pass input unchanged. |

## 14. Rulings made while writing this spec

1. **Fade length in frames, not ticks** (a format bump with migration at the region's start tempo). Fades belong to the audio, and region positions inside the media are already in frames.
2. *(Revised per user review.)* **The project has an audio format**, `Settings.format` = sample rate + bit depth (16-bit int, 24-bit int or 32-bit float). It is chosen at New (default 48 kHz / 24-bit), shown and changeable in Project Settings, and all media is stored in it. Changing it converts the media as one undo step. The engine processes float. Old projects migrate to the majority media rate and 24-bit, with mismatched media offline until converted. There is no real-time resampling: a device that won't switch mutes regions and offers to change the project rate.
3. **Files with more than two channels** split into mono media items, since the engine's buses are stereo.
4. **Offline media is runtime state**, never saved.
5. **Media commands move from `RoutingCmd` to a new `MediaCmd`**, with the existing semantics kept.
6. **Streaming sizes:** RAM cache for files of at most 4 MiB within a 512 MiB LRU budget; 256 voices × a 2 s ring; 3 s read-ahead; a 2 s loop-seam prefill; a 250 ms locate wait, then roll.
7. **An underrun plays silence and raises a counter.** It never stops the transport. A late start after the locate wait is reported separately.
8. **Separate input devices** are supported through a private aggregate device with drift correction (parent §6). Input opens only when needed.
9. **One input ring (4 s) and one writer thread.** Each armed audio lane gets its own CAF under `Media/Recorded/`. CAF data size −1 makes crash-truncated files valid.
10. **Pre-roll audio is kept as region handles.** Loop recording writes one file per lane per run, and each pass is a region slice.
11. **Audio overdub equals replace** under the parent overlap rule. To layer, use takes or other lanes.
12. **Passes become takes by default.** Without that setting, MIDI passes merge, but audio passes still become takes.
13. **Unreferenced media files go to the macOS Trash on save and close**, never unlinked. This single rule covers recording, compact and external edits.
14. **MIDI and automation timestamps are the audible position:** host time minus output latency.
15. **Lanes in referenced sequences cannot record.** Only lanes of the top-level playing sequence record.
16. **The strip automation mode is document state and undoable.** `Off` ignores automation.
17. **Write writes fader, pan, mute and sends continuously**, and plugin parameters only once touched. After Write, the strip falls back to Latch (a Setting).
18. **A loop wrap ends Touch, Latch and Write writing**, and the next cycle restarts in the strip's mode.
19. **Touch return time** defaults to 250 ms and the glide is written as a ramp. Gesture sources without touch release after 500 ms idle.
20. **Trim adds the delta** (in dB for gain targets) to the existing automation. Where none exists, it writes absolute points. Stepped targets treat Trim as Touch.
21. **Thinning** uses Ramer–Douglas–Peucker with a tolerance of 0.1 dB for gain targets and 0.25 % for others. Capture runs at 10 ms.
22. **Recorded automation goes into the first lane already automating that target**, or a new lane in the top-level sequence.
23. **Surface hardware settings and command learn maps** live in app preferences (`surfaces.json`). **Target learn maps** live in the project.
24. **MCU supports a main unit plus up to 3 extenders.** No fader feedback is sent while a fader is touched.
25. *(Revised per user review.)* **Bounce is offline only.** External MIDI destinations are silent, with a warning that points to Track Synth. Plugins are copies, so live playback continues. **Tracking a hardware synth as audio is an explicit use case** (§6.7): a `return_of` audio lane records the synth's return in the same pass as the MIDI that drives it, aligned by latency compensation plus a measured per-destination `return_latency_ms`. Real-time bounce and an external-instrument round trip are deferred.
26. **The external editor gets a WAV of the region span** (no gain or fades). The edit replaces that region's media, with offset 0 and the new length.
27. *(Revised per user review.)* **MIDI input effects (D26h) are included**: a per-lane chain of up to 8 built-ins (Arpeggiator, Transpose, Velocity, Chord, Input Quantize) on the real-time thread, active while the lane is armed or monitoring Input. Recording is post-effect, per main spec §6. Input Quantize affects only what is recorded, never live thru. Groove and plugin MIDI effects are deferred. This is a new plan, 5g, run last, because it depends only on 5c's input path.
28. **Normalize is non-destructive region gain**, computed from the overview peak.
29. **Default project rate is 48 kHz.** It is the common video and interface default, and its 24-bit pairing is the user's choice.
30. **16-bit media and bounces get TPDF dither. 24-bit is rounded without dither.** Integer formats clip at full scale and count the clipped samples.
31. **Synth return latency is per destination, in milliseconds**, so it is rate-independent. Measure uses 5 pings, onsets above −40 dBFS and the median, minus the HAL's reported round trip.
32. **The external editor's WAV is written in the project format**, and its return is converted back into it.
