import Foundation
import VizDAWCore

/// A scripted `SessionProtocol` for view-model tests.
final class FakeSession: SessionProtocol, @unchecked Sendable {
    var snap: DocSnapshot
    var laneTable: [UInt64: [LaneRow]] = [:]
    var state = TransportState(
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)
    var playError: VizError?
    var transportError: VizError?
    /// Thrown by `undo()` and `redo()`.
    var stepError: VizError?
    var savedPath: String?
    var lanesCalls = 0
    var renames: [(UInt64, String)] = []
    var mutes: [(seq: UInt64, lane: UInt64, mute: Bool)] = []
    var nextSnapshot: DocSnapshot?
    /// What the next `idle()` calls return, oldest first.
    var idleUpdates: [IdleUpdate] = []
    var idleCalls = 0
    var slots: [SlotRow] = []

    init(_ sequences: [SeqRow], version: UInt64 = 0) {
        snap = DocSnapshot(
            version: version, title: "Untitled", dirty: false, undoLabel: nil,
            redoLabel: nil, sequences: sequences, warnings: [], laneRevs: [])
    }

    /// Makes the next command return `s`.
    func bump(_ s: DocSnapshot) { nextSnapshot = s }

    private func commandResult() -> DocSnapshot {
        if let n = nextSnapshot {
            snap = n
            nextSnapshot = nil
        } else {
            snap.version += 1
        }
        return snap
    }

    func lanes(seq: UInt64) throws -> [LaneRow] {
        lanesCalls += 1
        guard let rows = laneTable[seq] else {
            throw VizError.Model(message: "unknown id seq\(seq)")
        }
        return rows
    }

    /// `locate` calls (plan 5f-1 Task 10).
    var locates: [Int64] = []
    func locate(tick: Int64) throws {
        locates.append(tick)
        state.tick = tick
    }
    func path() throws -> String? { savedPath }

    /// The bar's rewind and fast forward (plan 7d Task B1): clicks, held
    /// keys' steps, winds (quarter notes), every wind tried (counted
    /// before `jumpError` throws), and hold ends. A step is ±61 440 and a
    /// wind `round(q × 15 360)` from `state.tick`, floored at 0.
    var stepBars: [Bool] = []
    var keySteps: [Bool] = []
    var winds: [Double] = []
    var windAttempts = 0
    var holdEnds = 0
    /// Thrown by the steps and the wind when set.
    var jumpError: VizError?
    private func jump(by: Int64) -> Int64 {
        let (to, over) = state.tick.addingReportingOverflow(by)
        state.tick = max(0, over ? (by > 0 ? Int64.max : 0) : to)
        return state.tick
    }
    func transportStepBar(forward: Bool) throws -> Int64 {
        if let e = jumpError { throw e }
        stepBars.append(forward)
        return jump(by: forward ? 61_440 : -61_440)
    }
    func transportKeyStep(forward: Bool) throws -> Int64 {
        if let e = jumpError { throw e }
        keySteps.append(forward)
        return jump(by: forward ? 61_440 : -61_440)
    }
    func transportWind(quarters: Double) throws -> Int64 {
        windAttempts += 1
        if let e = jumpError { throw e }
        winds.append(quarters)
        let by = (quarters * 15_360).rounded()
        let limit = Double(Int64.max)
        if !by.isFinite { return jump(by: 0) }
        return jump(by: by >= limit ? .max : by <= -limit ? .min : Int64(by))
    }
    func transportHoldEnd() throws -> Int64 {
        holdEnds += 1
        return state.tick
    }

    func play(seq: UInt64) throws {
        if let e = playError { throw e }
        state.playing = true
        state.seq = seq
    }

    /// `playFrom` calls: sequence, start and end (plan 4f).
    var playsFrom: [(seq: UInt64, from: Int64, to: Int64?)] = []
    func playFrom(seq: UInt64, from: Int64, to: Int64?) throws {
        if let e = playError { throw e }
        playsFrom.append((seq, from, to))
        state.playing = true
        state.seq = seq
        state.tick = from
    }

    /// What `selectionSpan` answers.
    var span: TickRange?
    func selectionSpan(seq: UInt64) throws -> TickRange? { span }

    /// Marker ticks `markerStep` steps between.
    var markerTicks: [Int64] = []
    func markerStep(seq: UInt64, tick: Int64, forward: Bool) throws -> Int64? {
        forward ? markerTicks.filter { $0 > tick }.min() : markerTicks.filter { $0 < tick }.max()
    }

    var armMoves: [(seq: UInt64, forward: Bool)] = []
    var armError: VizError?
    func moveRecordArm(seq: UInt64, forward: Bool) throws -> DocSnapshot {
        if let e = armError { throw e }
        armMoves.append((seq, forward))
        return commandResult()
    }

    /// `stepEntry` calls (plan 4f) and how they are answered: by
    /// `stepAnswer`, else a quarter note forward for notes, rests, ties
    /// and references (with a new snapshot for notes and references), a
    /// value key sets the value.
    var stepCalls: [(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow)] = []
    var stepEntryError: VizError?
    var stepAnswer: ((StepStateRow, StepInputRow) -> StepResultRow)?
    func stepEntry(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow) throws -> StepResultRow {
        if let e = stepEntryError { throw e }
        stepCalls.append((seq, lane, state, input))
        if let a = stepAnswer { return a(state, input) }
        var s = state
        switch input {
        case .notes, .reference:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: commandResult(), moved: true)
        case .rest, .tie:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: nil, moved: true)
        case .value(let v):
            s.value = v
            return StepResultRow(state: s, snapshot: nil, moved: false)
        default:
            return StepResultRow(state: s, snapshot: nil, moved: false)
        }
    }

    var undoCalls = 0
    var redoCalls = 0
    func redo() throws -> DocSnapshot {
        if let e = stepError { throw e }
        redoCalls += 1
        return commandResult()
    }

    func renameSequence(seq: UInt64, name: String) throws -> DocSnapshot {
        renames.append((seq, name))
        return commandResult()
    }

    func save() throws {
        guard savedPath != nil else { throw VizError.Io(message: "use Save As") }
        snap.dirty = false
    }

    /// A Save came while recording (plan 5d-1 Ruling 19).
    var savesAfterPassRow = false

    func savesAfterPass() throws -> Bool { savesAfterPassRow }

    /// What `addMidiDestination` was asked (plan 5d-1 Ruling 25).
    var addedDestinations: [(name: String, port: String, channel: UInt8)] = []

    func addMidiDestination(name: String, port: String, channel: UInt8) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        addedDestinations.append((name, port, channel))
        dests.append(DestRow(id: UInt64(100 + addedDestinations.count), name: name, plugin: nil, external: true))
        return snap
    }

    func saveAs(path: String) throws {
        savedPath = path
        snap.dirty = false
        snap.title = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
    }

    func setLaneMute(seq: UInt64, lane: UInt64, mute: Bool) throws -> DocSnapshot {
        mutes.append((seq, lane, mute))
        return commandResult()
    }

    var loopError: VizError?
    func setLoop(on: Bool) throws { if let e = loopError { throw e } }
    func snapshot() throws -> DocSnapshot { snap }
    func stop() throws {
        state.playing = false
        recordRow.recording = false
    }
    func transport() throws -> TransportState {
        if let e = transportError { throw e }
        return state
    }
    func undo() throws -> DocSnapshot {
        if let e = stepError { throw e }
        undoCalls += 1
        return commandResult()
    }

    func idle() throws -> IdleUpdate {
        idleCalls += 1
        if let u = idleUpdates.first {
            idleUpdates.removeFirst()
            if let s = u.snapshot { snap = s }
            return u
        }
        return IdleUpdate(snapshot: nil, slotsChanged: false, paramsChanged: [], problems: [])
    }

    func pluginSlots() throws -> [SlotRow] { slots }

    // MARK: Mixer

    var mixerSnap = MixerSnapshot(version: 0, strips: [], buses: [])
    var levelsValue = MixerLevels(
        meters: [], dspLoad: 0, dspPeak: 0, latencyFrames: 0, faulted: [])
    var levelsCalls = 0
    /// Every mixer call, as text (`"setGain 1 -3.0"`).
    var calls: [String] = []
    var mixerError: VizError?

    private func mixerCall(_ text: String) throws -> DocSnapshot {
        if let e = mixerError { throw e }
        calls.append(text)
        return commandResult()
    }

    var mixerCalls = 0
    /// Thrown by `mixer()` only (a busy session while an editor opens).
    var mixerFetchError: VizError?
    func mixer() throws -> MixerSnapshot {
        mixerCalls += 1
        if let e = mixerFetchError { throw e }
        return mixerSnap
    }
    func levels() throws -> MixerLevels {
        levelsCalls += 1
        return levelsValue
    }
    var beginError: VizError?
    func beginEdit(label: String) throws {
        if let e = beginError { throw e }
        calls.append("begin \(label)")
    }
    func endEdit() throws -> DocSnapshot { try mixerCall("end") }
    func addBus(name: String) throws -> DocSnapshot { try mixerCall("addBus \(name)") }
    func removeBus(strip: UInt64) throws -> DocSnapshot { try mixerCall("removeBus \(strip)") }
    func renameStrip(strip: UInt64, name: String) throws -> DocSnapshot {
        try mixerCall("renameStrip \(strip) \(name)")
    }
    func moveStrip(strip: UInt64, index: UInt32) throws -> DocSnapshot {
        try mixerCall("moveStrip \(strip) \(index)")
    }
    func setGain(strip: UInt64, gainDb: Double) throws -> DocSnapshot {
        try mixerCall("setGain \(strip) \(gainDb)")
    }
    func setPan(strip: UInt64, pan: Double) throws -> DocSnapshot {
        try mixerCall("setPan \(strip) \(pan)")
    }
    func setStripMute(strip: UInt64, mute: Bool) throws -> DocSnapshot {
        try mixerCall("mute \(strip) \(mute)")
    }
    func setStripSolo(strip: UInt64, solo: Bool) throws -> DocSnapshot {
        try mixerCall("solo \(strip) \(solo)")
    }
    func setOutput(strip: UInt64, bus: UInt64?) throws -> DocSnapshot {
        try mixerCall("output \(strip) \(bus.map(String.init) ?? "master")")
    }
    func addSend(strip: UInt64, target: UInt64) throws -> DocSnapshot {
        try mixerCall("addSend \(strip) \(target)")
    }
    func editSend(strip: UInt64, index: UInt32, send: SendRow) throws -> DocSnapshot {
        try mixerCall(
            "editSend \(strip) \(index) \(send.target) \(send.levelDb) \(send.preFader) \(send.enabled)")
    }
    func removeSend(strip: UInt64, index: UInt32) throws -> DocSnapshot {
        try mixerCall("removeSend \(strip) \(index)")
    }
    func addInsert(strip: UInt64, index: UInt32?, plugin: PluginRefRow) throws -> DocSnapshot {
        try mixerCall("addInsert \(strip) \(index.map(String.init) ?? "end") \(plugin.id)")
    }
    func removeInsert(slot: UInt64) throws -> DocSnapshot { try mixerCall("removeInsert \(slot)") }
    func moveInsert(slot: UInt64, strip: UInt64, index: UInt32) throws -> DocSnapshot {
        try mixerCall("moveInsert \(slot) \(strip) \(index)")
    }
    func setBypass(slot: UInt64, bypass: Bool) throws -> DocSnapshot {
        try mixerCall("bypass \(slot) \(bypass)")
    }
    /// Loads or empties the fake strip's EQ (plan 7b Ruling 7): an empty
    /// slot cannot be emptied, as the core refuses it.
    func setEq(strip: UInt64, plugin: PluginRefRow?) throws -> DocSnapshot {
        if let e = mixerError { throw e }
        guard let i = mixerSnap.strips.firstIndex(where: { $0.id == strip }) else {
            throw VizError.Model(message: "unknown id strip\(strip)")
        }
        if plugin == nil, mixerSnap.strips[i].eq == nil {
            throw VizError.Model(message: "the EQ slot is empty")
        }
        let r = try mixerCall("setEq \(strip) \(plugin?.id ?? "none")")
        mixerSnap.strips[i].eq = plugin.map { p in
            var s = slotRow(900 + strip, p.name)
            s.plugin = p
            return s
        }
        return r
    }
    /// Sets the fake strip's own colour (controller ruling C-1).
    func setStripColour(strip: UInt64, colour: SwatchRow?) throws -> DocSnapshot {
        let r = try mixerCall("setStripColour \(strip) \(colour.map { "\($0)" } ?? "none")")
        if let i = mixerSnap.strips.firstIndex(where: { $0.id == strip }) {
            mixerSnap.strips[i].colour = colour
        }
        return r
    }
    func setSidechain(slot: UInt64, source: UInt64?) throws -> DocSnapshot {
        try mixerCall("sidechain \(slot) \(source.map(String.init) ?? "none")")
    }
    func addInstrumentOutput(dest: UInt64, index: UInt16, name: String) throws -> DocSnapshot {
        try mixerCall("addOutput \(dest) \(index) \(name)")
    }
    func addPluginInstrument(name: String, plugin: PluginRefRow) throws -> DocSnapshot {
        try mixerCall("addInstrument \(name) \(plugin.id)")
    }
    var dests: [DestRow] = []
    var destinationsError: VizError?
    func destinations() throws -> [DestRow] {
        if let e = destinationsError { throw e }
        return dests
    }
    func setDestinationPlugin(dest: UInt64, plugin: PluginRefRow) throws -> DocSnapshot {
        try mixerCall("destPlugin \(dest) \(plugin.id)")
    }

    // MARK: Parameters

    var paramTable: [UInt64: [ParamRow]] = [:]
    /// Live sets, in order: (slot, param, value).
    var liveSets: [(UInt64, UInt64, Double)] = []
    var endedGestures: [UInt64] = []

    func params(slot: UInt64) throws -> [ParamRow] {
        if readBusy { throw VizError.Busy(message: "plugin host is busy") }
        return paramTable[slot] ?? []
    }
    func setParam(slot: UInt64, param: UInt64, value: Double) throws {
        liveSets.append((slot, param, value))
    }
    func commitParams(slot: UInt64, values: [ParamValue]) throws -> DocSnapshot {
        try mixerCall("commitParams \(slot) " + values.map { "\($0.id)=\($0.value)" }.joined(separator: ","))
    }
    func endPluginGestures(slot: UInt64) throws { endedGestures.append(slot) }

    // MARK: Plugin windows

    /// Editor sizes by slot; slots not listed have no editor.
    var editors: [UInt64: EditorSize] = [:]
    var openedEditors: [UInt64] = []
    var closedEditors: [UInt64] = []
    var windowsOpen: Set<UInt64> = []

    /// openEditor throws Busy this many more times before working.
    var openBusy = 0
    /// mixer() and params() throw Busy while true.
    var readBusy = false
    func openEditor(slot: UInt64, parent: UInt64) throws -> EditorSize {
        if openBusy > 0 {
            openBusy -= 1
            throw VizError.Busy(message: "plugin host is busy")
        }
        guard let e = editors[slot] else {
            throw VizError.Engine(message: "editor unavailable: the plugin has no editor")
        }
        openedEditors.append(slot)
        return e
    }
    func resizeEditor(slot: UInt64, width: UInt32, height: UInt32) throws -> EditorSize {
        guard let e = editors[slot] else {
            throw VizError.Engine(message: "editor unavailable: the plugin has no editor")
        }
        return e.resizable ? EditorSize(width: width, height: height, resizable: true) : e
    }
    /// closeEditor throws Busy this many more times before succeeding.
    var closeBusy = 0
    func closeEditor(slot: UInt64) throws {
        if closeBusy > 0 {
            closeBusy -= 1
            throw VizError.Busy(message: "another editor is opening")
        }
        closedEditors.append(slot)
    }
    func setWindowOpen(slot: UInt64, open: Bool) throws {
        if open { windowsOpen.insert(slot) } else { windowsOpen.remove(slot) }
    }
    func openWindows() throws -> [UInt64] { windowsOpen.sorted() }

    // MARK: Safe mode and closing

    var held: [UInt64] = []
    var loadedPlugins: [[UInt64]] = []
    var closeCalls = 0

    func loadPlugins(slots: [UInt64]) throws {
        loadedPlugins.append(slots)
        held.removeAll { slots.contains($0) }
    }
    func heldSlots() throws -> [UInt64] { held }
    func close() throws { closeCalls += 1 }
    /// What `overview` and `regionWaveform` answer (nil: pending silence).
    var waveformRows: OverviewRows?
    /// `regionWaveform` calls: event, from, to, columns.
    var waveformCalls: [(event: UInt64, from: Int64, to: Int64, columns: UInt32)] = []
    /// `overview` calls: media, from, to, columns (plan 7c Task 6).
    var overviewCalls: [(media: UInt64, from: UInt64, to: UInt64, columns: UInt32)] = []
    func overview(media: UInt64, fromFrame: UInt64, toFrame: UInt64, columns: UInt32) throws -> OverviewRows {
        overviewCalls.append((media, fromFrame, toFrame, columns))
        return rows(columns: columns)
    }
    private func rows(columns: UInt32) -> OverviewRows {
        waveformRows ?? OverviewRows(
            state: .pending, channels: 1, columns: columns, min: Array(repeating: 0, count: Int(columns)),
            max: Array(repeating: 0, count: Int(columns)))
    }
    func regionWaveform(event: UInt64, fromTick: Int64, toTick: Int64, columns: UInt32) throws -> OverviewRows {
        waveformCalls.append((event, fromTick, toTick, columns))
        return rows(columns: columns)
    }
    /// What `regionEnvelope` answers (nil: `columns` ones).
    var envelope: [Float]?
    /// `regionEnvelope` calls: event, from, to, columns.
    var envelopeCalls: [(event: UInt64, from: UInt64, to: UInt64, columns: UInt32)] = []
    func regionEnvelope(event: UInt64, fromFrame: UInt64, toFrame: UInt64, columns: UInt32) throws -> [Float] {
        envelopeCalls.append((event, fromFrame, toFrame, columns))
        return envelope ?? Array(repeating: 1, count: Int(columns))
    }
    // MARK: Live input (plan 5c Task 8)

    var injected: [(port: String, bytes: Data)] = []
    var inputRow = InputInfoRow(
        device: "", channels: 0, names: [], inputLatencyFrames: 0, outputLatencyFrames: 0)

    var injectError: VizError?

    func injectMidi(port: String, bytes: Data) throws -> Bool {
        if let e = injectError { throw e }
        injected.append((port, bytes))
        return true
    }

    func inputInfo() throws -> InputInfoRow { inputRow }

    var inputLevelsRow: [Float] = []

    /// Peaks are read and reset, as the core's are, when set (plan 5d-2 pre-flight I2).
    var inputLevelsReset = false

    func inputLevels() throws -> [Float] {
        defer { if inputLevelsReset { inputLevelsRow = inputLevelsRow.map { _ in 0 } } }
        return inputLevelsRow
    }

    // MARK: Recording (plan 5c Task 9)

    var recordCalls: [(seq: UInt64, spec: RecordSpecRow)] = []
    var recordError: VizError?
    var passPresses: [Bool] = []
    var recordRow = RecordStateRow(
        recording: false, seq: nil, spec: nil, pass: 0, kept: 0, discarded: 0, lanes: [], frames: 0,
        overflows: 0, gapFrames: 0, freeBytes: nil, recovered: [])

    func record(seq: UInt64, spec: RecordSpecRow) throws {
        if let e = recordError { throw e }
        recordCalls.append((seq, spec))
        recordRow.recording = true
        recordRow.seq = seq
        recordRow.spec = spec
        state.playing = true
        state.seq = seq
    }

    func keepPass() throws { passPresses.append(true) }
    func discardPass() throws { passPresses.append(false) }
    var recordStateError: VizError?
    var recordStateCalls = 0
    /// Thrown once, then cleared, by `recordState`.
    var recordStateErrorOnce: VizError?
    /// What `recordingWaveform` answers (plan 5d-1 Ruling 21); nil: silent.
    var recordingRows: OverviewRows?
    var recordingWaveformCalls: [(lane: UInt64, from: UInt64, to: UInt64, columns: UInt32)] = []

    func recordingWaveform(lane: UInt64, fromFileFrame: UInt64, toFileFrame: UInt64, columns: UInt32) throws
        -> OverviewRows
    {
        recordingWaveformCalls.append((lane, fromFileFrame, toFileFrame, columns))
        return recordingRows
            ?? OverviewRows(
                state: .ready, channels: 1, columns: columns, min: Array(repeating: 0, count: Int(columns)),
                max: Array(repeating: 0, count: Int(columns)))
    }

    func recordState() throws -> RecordStateRow {
        recordStateCalls += 1
        if let e = recordStateError { throw e }
        if let e = recordStateErrorOnce {
            recordStateErrorOnce = nil
            throw e
        }
        return recordRow
    }

    var recovered: [RecoveredFileRow] = []
    var recoverAnswers: [Bool] = []

    func recoveredRecordings() throws -> [String] { recovered.map(\.path) }
    func recoveredFiles() throws -> [RecoveredFileRow] { recovered }

    /// Thrown (once, then cleared) by `recoverRecordings`.
    var recoverError: VizError?

    func recoverRecordings(import: Bool) throws -> DocSnapshot {
        if let e = recoverError {
            recoverError = nil
            throw e
        }
        recoverAnswers.append(`import`)
        recovered = []
        return commandResult()
    }

    // MARK: Media pool (plan 5b Task 6)

    var renamed: [(UInt64, String)] = []
    var removedMedia: [UInt64] = []
    var compactions = 0
    var relinks: [(media: UInt64, path: String)] = []
    var searches: [String] = []
    /// What `mediaUsers` answers, per item.
    var usersTable: [UInt64: [MediaUseRow]] = [:]
    /// Thrown by the pool's edits.
    var poolError: VizError?
    func renameMedia(media: UInt64, name: String) throws -> DocSnapshot {
        if let e = poolError { throw e }
        renamed.append((media, name))
        return commandResult()
    }
    func removeMedia(media: UInt64) throws -> DocSnapshot {
        if let e = poolError { throw e }
        removedMedia.append(media)
        return commandResult()
    }
    func compactMedia() throws -> DocSnapshot {
        if let e = poolError { throw e }
        compactions += 1
        return commandResult()
    }
    func mediaUsers(media: UInt64) throws -> [MediaUseRow] { usersTable[media] ?? [] }
    func mediaFile(media: UInt64) throws -> String { "/tmp/pool/\(media).caf" }
    func relinkMedia(media: UInt64, path: String) throws -> UInt64 {
        relinks.append((media, path))
        let id = UInt64(3000 + relinks.count)
        jobRows[id] = JobRow(
            id: id, kind: .relink, progress: 1, state: jobStartState, message: nil, problems: [], created: [media])
        return id
    }
    func searchMedia(folder: String) throws -> UInt64 {
        searches.append(folder)
        let id = UInt64(4000 + searches.count)
        jobRows[id] = JobRow(
            id: id, kind: .relink, progress: 1, state: jobStartState, message: nil, problems: [], created: [])
        return id
    }

    /// `normalizeRegions` calls; `normalizeError` is thrown instead.
    var normalizations: [(events: [UInt64], peakDb: Float)] = []
    var normalizeError: VizError?
    func normalizeRegions(events: [UInt64], peakDb: Float) throws -> DocSnapshot {
        if let e = normalizeError { throw e }
        normalizations.append((events, peakDb))
        return commandResult()
    }
    var mediaRows: [MediaRow] = []
    var jobRows: [UInt64: JobRow] = [:]
    var imports: [([String], ImportTargetRow?)] = []
    var mediaError: VizError?
    func media() throws -> [MediaRow] {
        if let e = mediaError { throw e }
        return mediaRows
    }
    func recheckMedia() throws -> Bool { false }
    func importAudio(paths: [String], target: ImportTargetRow?) throws -> UInt64 {
        imports.append((paths, target))
        let id = UInt64(imports.count)
        jobRows[id] = JobRow(
            id: id, kind: .import, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState,
            message: nil, problems: [], created: [])
        return id
    }
    var jobCalls = 0
    var jobBusy = false
    func job(id: UInt64) throws -> JobRow {
        jobCalls += 1
        if jobBusy { throw VizError.Busy(message: "busy") }
        guard let j = jobRows[id] else { throw VizError.Model(message: "unknown job \(id)") }
        return j
    }
    var cancelled: [UInt64] = []
    var bounces: [BounceRequestRow] = []
    var bounceError: VizError?
    var checkRow = BounceCheckRow(externalLanes: 0, frames: 96_000, files: [], warning: nil)
    var checkRequests: [BounceRequestRow] = []
    var resultRow: BounceResultRow?
    func bounce(req: BounceRequestRow) throws -> UInt64 {
        if let e = bounceError { throw e }
        bounces.append(req)
        let id = UInt64(3000 + bounces.count)
        jobRows[id] = JobRow(
            id: id, kind: .bounce, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState,
            message: nil, problems: [], created: [])
        return id
    }
    var checkError: VizError?
    func bounceCheck(req: BounceRequestRow) throws -> BounceCheckRow {
        if let e = checkError { throw e }
        checkRequests.append(req)
        return checkRow
    }
    func bounceResult(job: UInt64) throws -> BounceResultRow? { resultRow }

    // MARK: External sample editor (plan 5f-3)

    /// One `applyExternalEdit` call.
    struct ExternalApply: Equatable {
        let event: UInt64
        let path: String
    }
    /// What `openExternal` answers (nil: a row for the event under
    /// /tmp/p.vizdaw), and its refusal.
    var externalRow: ExternalEditRow?
    var externalError: VizError?
    /// The regions `openExternal` was called for (refused calls too).
    var externalOpened: [UInt64] = []
    /// What `externalEdit` answers per region: the copy `openExternal`
    /// made, else `.noEdit`.
    var externalStates: [UInt64: ExternalEditStateRow] = [:]
    /// `applyExternalEdit` calls, and its refusal.
    var externalApplies: [ExternalApply] = []
    var applyError: VizError?
    func openExternal(event: UInt64) throws -> ExternalEditRow {
        externalOpened.append(event)
        if let e = externalError { throw e }
        let row = externalRow ?? ExternalEditRow(event: event, path: "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav", from: 1)
        externalStates[event] = .copy(row: row)
        return row
    }
    func externalEdit(event: UInt64) throws -> ExternalEditStateRow { externalStates[event] ?? .noEdit }
    func applyExternalEdit(event: UInt64, path: String) throws -> UInt64 {
        if let e = applyError { throw e }
        externalApplies.append(ExternalApply(event: event, path: path))
        let id = UInt64(5000 + externalApplies.count)
        jobRows[id] = JobRow(
            id: id, kind: .externalEdit, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState,
            message: nil, problems: [], created: [])
        return id
    }
    var jobStartState: JobStateRow = .done
    func cancelJob(id: UInt64) throws { cancelled.append(id) }
    var info = ProjectInfoRow(
        format: ProjectFormatRow(sampleRate: 48_000, depth: .int24), mismatched: 0, rateMismatch: false,
        deviceRate: 0)
    var formatChanges: [ProjectFormatRow] = []
    var conversions = 0
    func projectInfo() throws -> ProjectInfoRow { info }
    func setProjectFormat(format: ProjectFormatRow) throws -> UInt64 {
        formatChanges.append(format)
        info.format = format
        let id = UInt64(1000 + formatChanges.count)
        jobRows[id] = JobRow(id: id, kind: .format, progress: 1, state: .done, message: nil, problems: [], created: [])
        return id
    }
    func convertMedia() throws -> UInt64 {
        conversions += 1
        info.mismatched = 0
        let id = UInt64(2000 + conversions)
        jobRows[id] = JobRow(id: id, kind: .convert, progress: 1, state: .done, message: nil, problems: [], created: [])
        return id
    }

    // MARK: Editor reads (plan 4a)

    /// Lane events by `"seq/lane"`.
    var laneEventTable: [String: LaneEvents] = [:]
    var laneEventsCalls: [String] = []
    /// Thrown by every editor read (`laneEvents`, `blocks`, `sequenceInfo`).
    var readError: VizError?
    func laneEvents(seq: UInt64, lane: UInt64) throws -> LaneEvents {
        laneEventsCalls.append("\(seq)/\(lane)")
        if let e = readError { throw e }
        guard let ev = laneEventTable["\(seq)/\(lane)"] else {
            throw VizError.Model(message: "unknown lane \(lane)")
        }
        return ev
    }

    /// Drum lanes by sequence (plan 4e); a sequence not listed has none.
    var drumTable: [UInt64: [DrumLaneRow]] = [:]
    var drumLanesCalls = 0
    /// When set, answers `drumLanes` instead of the table.
    var drumReadHook: (() throws -> [DrumLaneRow])?
    func drumLanes(seq: UInt64) throws -> [DrumLaneRow] {
        drumLanesCalls += 1
        if let h = drumReadHook { return try h() }
        if let e = readError { throw e }
        return drumTable[seq] ?? []
    }

    var infoTable: [UInt64: SequenceInfo] = [:]
    var infoCalls = 0
    /// Thrown by `secondsAt`.
    var secondsAtError: VizError?
    func secondsAt(seq: UInt64, ticks: [Int64]) throws -> [Double] {
        if let e = secondsAtError { throw e }
        return ticks.map { Double($0) / 30_720 }
    }
    func ticksAt(seq: UInt64, seconds: [Double]) throws -> [Int64] { seconds.map { Int64(($0 * 30_720).rounded()) } }

    // MARK: Select-and-modify and the list (plan 4c)

    var lastModifyRow: ModifyRow?
    var modifyTemplateRows: [ModifyTemplateRow] = []
    /// Thrown by `lastModify()` and `modifyTemplates()`.
    var modifyReadError: VizError?
    var modifyReads = 0
    func lastModify() throws -> ModifyRow? {
        modifyReads += 1
        if let e = modifyReadError { throw e }
        return lastModifyRow
    }
    func modifyTemplates() throws -> [ModifyTemplateRow] {
        if let e = modifyReadError { throw e }
        return modifyTemplateRows
    }

    func sequenceInfo(seq: UInt64) throws -> SequenceInfo {
        infoCalls += 1
        if let e = readError { throw e }
        return infoTable[seq] ?? fourFour(seq)
    }

    /// Blocks by `"seq/lane"`.
    var blocksTable: [String: [BlockRow]] = [:]
    var blocksCalls: [String] = []
    var lastRule: BlockRule?
    func blocks(seq: UInt64, lane: UInt64, rule: BlockRule) throws -> [BlockRow] {
        blocksCalls.append("\(seq)/\(lane)")
        lastRule = rule
        if let e = readError { throw e }
        return blocksTable["\(seq)/\(lane)"] ?? []
    }

    // MARK: Editor edits (plan 4a)

    var edits: [EditCommand] = []
    var editError: VizError?
    var editFilters: [ViewFilterRow?] = []
    func edit(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editError { throw e }
        edits.append(cmd)
        editFilters.append(filter)
        calls.append("edit")
        return commandResult()
    }

    /// `editAll` calls: their labels and commands.
    var allEdits: [(label: String, cmds: [EditCommand])] = []
    var editAllError: VizError?
    func editAll(label: String, cmds: [EditCommand], filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editAllError { throw e }
        allEdits.append((label, cmds))
        edits.append(contentsOf: cmds)
        calls.append("editAll \(label)")
        return commandResult()
    }

    var gestureEdits: [EditCommand] = []
    /// What the next `editGesture` calls throw, oldest first (nil succeeds).
    var gestureErrors: [VizError?] = []
    func editGesture(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        let e = gestureErrors.isEmpty ? nil : gestureErrors.removeFirst()
        if let e {
            calls.append("gesture refused")
            throw e
        }
        gestureEdits.append(cmd)
        calls.append("gesture")
        return commandResult()
    }

    /// abortEdit throws these, front first, before working.
    var abortErrors: [VizError] = []
    func abortEdit() throws -> DocSnapshot {
        calls.append("abort")
        if !abortErrors.isEmpty { throw abortErrors.removeFirst() }
        return commandResult()
    }

    // MARK: Selection, templates, layout (plan 4a)

    var sel = SelectionRow(timeRange: nil, lanes: [], events: [])
    var selectionSets: [SelectionRow] = []
    func selection() throws -> SelectionRow { sel }
    var selectionError: VizError?
    /// What the session keeps of a selection (Rust drops unknown lanes and events).
    var keepSelection: (SelectionRow) -> SelectionRow = { $0 }
    func setSelection(selection: SelectionRow) throws {
        if let e = selectionError { throw e }
        selectionSets.append(selection)
        sel = keepSelection(selection)
    }

    var templates: [(name: String, template: SequenceTemplate)] = []
    func newSequenceFromTemplate(name: String, template: SequenceTemplate) throws -> DocSnapshot {
        templates.append((name, template))
        return commandResult()
    }

    var layout: String?
    func uiLayout() throws -> String? { layout }
    func setUiLayout(json: String) throws { layout = json }

    var auditions: [(dest: UInt64, pitch: UInt8, on: Bool)] = []
    /// What the next `audition` calls throw, oldest first (nil succeeds).
    var auditionErrors: [VizError?] = []
    /// What a successful `audition` returns (false: no engine yet).
    var auditionSent = true
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        let e = auditionErrors.isEmpty ? nil : auditionErrors.removeFirst()
        if let e { throw e }
        auditions.append((dest, pitch, on))
        return auditionSent
    }

    // MARK: Synth tracking (plan 5c-2)

    var trackSynthCalls: [(seq: UInt64, lane: UInt64, first: UInt16, stereo: Bool, armMidi: Bool, muteAfter: Bool)] = []
    var returnLatencies: [(dest: UInt64, ms: Float)] = []
    /// Thrown (once, then cleared) by the synth-tracking calls.
    var synthError: VizError?

    func trackSynth(
        seq: UInt64, lane: UInt64, first: UInt16, stereo: Bool, armMidi: Bool, muteAfter: Bool
    ) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        trackSynthCalls.append((seq, lane, first, stereo, armMidi, muteAfter))
        return commandResult()
    }

    func setReturnLatency(dest: UInt64, ms: Float) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        returnLatencies.append((dest, ms))
        return commandResult()
    }

    var measures: [UInt64] = []
    /// What `measureReturn` finds.
    var measured: Float = 25

    /// Whether each Measure ran on the main thread (plan 5d-2: never).
    var measuredOnMain: [Bool] = []

    func measureReturn(dest: UInt64) throws -> MeasureRow {
        measuredOnMain.append(Thread.isMainThread)
        if let e = synthError {
            synthError = nil
            throw e
        }
        measures.append(dest)
        return MeasureRow(returnLatencyMs: measured, onsets: 5, snapshot: commandResult())
    }

    // MARK: Automation (plan 5e-1)

    var gestures: [(AutomationTargetRow, GesturePhaseRow)] = []
    /// Thrown by `gesture` while set.
    var gestureError: VizError?
    /// Thrown by `gesture` for value phases only, while set.
    var valueError: VizError?
    /// Thrown by `automationState` while set.
    var automationError: VizError?
    var automationRow = AutomationStateRow(modes: [], touched: [], suspended: false, writing: false)
    var modeCalls: [([UInt64], AutomationModeRow)] = []
    var previewRows: [PreviewPointRow] = []

    func gesture(target: AutomationTargetRow, phase: GesturePhaseRow) throws {
        if let e = gestureError { throw e }
        if case .value = phase, let e = valueError { throw e }
        gestures.append((target, phase))
    }

    /// The holders of `gestureFrom` calls, in order (plan 5f-1 Ruling 2).
    var holders: [GestureHolderRow] = []
    func gestureFrom(target: AutomationTargetRow, phase: GesturePhaseRow, holder: GestureHolderRow) throws {
        if let e = gestureError { throw e }
        if case .value = phase, let e = valueError { throw e }
        gestures.append((target, phase))
        holders.append(holder)
    }

    // MARK: Surfaces (plan 5f-1)

    var focusCalls = 0
    func focusSurfaces() throws { focusCalls += 1 }

    /// What the surface and learn methods were called with (Task 8).
    var surfaceSelections: [[UInt64]] = []
    var surfaceInjections: [(UInt32, Data)] = []
    var surfaceEvents: [SurfaceEventRow] = []
    var learnTargets: [AutomationTargetRow] = []
    var learnMapRows: [LearnMapRow] = []
    var learnModes: [(LearnSourceRow, LearnModeRow)] = []
    var learnPickups: [(LearnSourceRow, Bool)] = []
    var removedLearns: [LearnSourceRow] = []
    /// Thrown by `setLearnMode`, `setLearnPickup` and `removeLearn` (Task 11).
    var learnError: VizError?
    var unlearnTargets: [AutomationTargetRow] = []
    func setSurfaceSelection(strips: [UInt64]) throws { surfaceSelections.append(strips) }
    /// What `setSurfaceCountIn` was called with (plan 5f-4 Task 7).
    var surfaceCountIns: [Bool] = []
    /// Thrown by `setSurfaceCountIn` (after it is recorded) when set.
    var surfaceCountInError: VizError?
    func setSurfaceCountIn(on: Bool) throws {
        surfaceCountIns.append(on)
        if let e = surfaceCountInError { throw e }
    }
    func injectSurface(surface: UInt32, bytes: Data) throws { surfaceInjections.append((surface, bytes)) }
    func surfaceState() throws -> SurfaceStateRow {
        SurfaceStateRow(surfaces: [], feedback: [], learning: nil, problems: [])
    }
    func takeSurfaceEvents() throws -> [SurfaceEventRow] {
        defer { surfaceEvents = [] }
        return surfaceEvents
    }
    func learnTarget(target: AutomationTargetRow) throws { learnTargets.append(target) }
    func learnMaps() throws -> [LearnMapRow] { learnMapRows }
    func setLearnMode(source: LearnSourceRow, mode: LearnModeRow) throws -> DocSnapshot {
        if let e = learnError { throw e }
        learnModes.append((source, mode))
        return commandResult()
    }
    func setLearnPickup(source: LearnSourceRow, on: Bool) throws -> DocSnapshot {
        if let e = learnError { throw e }
        learnPickups.append((source, on))
        return commandResult()
    }
    func removeLearn(source: LearnSourceRow) throws -> DocSnapshot {
        if let e = learnError { throw e }
        removedLearns.append(source)
        return commandResult()
    }
    func unlearnTarget(target: AutomationTargetRow) throws -> DocSnapshot {
        unlearnTargets.append(target)
        return commandResult()
    }

    /// Thrown by `setAutomationMode` (before it is recorded).
    var modeError: VizError?
    func setAutomationMode(strips: [UInt64], mode: AutomationModeRow) throws -> DocSnapshot {
        if let e = modeError { throw e }
        modeCalls.append((strips, mode))
        return commandResult()
    }

    func automationState() throws -> AutomationStateRow {
        if let e = automationError { throw e }
        return automationRow
    }

    func automationPreview(target: AutomationTargetRow) throws -> [PreviewPointRow] { previewRows }
}

func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
    SeqRow(
        id: id, name: name, useCount: uses, topLevel: topLevel, lengthTicks: 61_440,
        laneCount: 1, comment: "", keyboardSlot: nil)
}

func lane(_ id: UInt64, _ name: String, mute: Bool = false) -> LaneRow {
    LaneRow(
        id: id, name: name, destination: nil, midiEventCount: 0, regionCount: 0,
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false, arm: false, midiDest: nil, audioOut: nil, input: nil,
        monitor: .auto)
}

/// Song (top-level) using Groove, which uses Beat.
func demoFake() -> FakeSession {
    let f = FakeSession([
        seq(1, "Beat", topLevel: false, uses: 1),
        seq(2, "Groove", topLevel: false, uses: 2),
        seq(3, "Song", topLevel: true),
    ])
    f.laneTable = [1: [lane(10, "Drums")], 2: [lane(20, "Drums"), lane(21, "Bass")],
                   3: [lane(30, "Groove"), lane(31, "Chords")]]
    return f
}

func slotRow(_ id: UInt64, _ name: String, status: SlotStatus = .live, sidechain: Bool = false) -> SlotRow {
    SlotRow(
        id: id, plugin: PluginRefRow(format: .clap, id: "x.\(name)", name: name, vendor: "V", version: "1"),
        instrument: false, bypass: false, sidechain: nil, status: status, latency: 0,
        hasSidechainInput: sidechain, outputs: 1, hasEditor: false)
}

func stripRow(
    _ id: UInt64, _ name: String, kind: StripKindRow = .bus, inserts: [SlotRow] = [],
    sends: [SendRow] = [], output: UInt64? = nil
) -> StripRow {
    StripRow(
        id: id, name: name, kind: kind, kindLabel: "\(kind)", instrument: nil, inserts: inserts,
        gainDb: 0, pan: 0, mute: false, solo: false, soloMuted: false, sends: sends,
        output: output, freeOutputs: [], automation: .read, eq: nil,
        kindColour: kindColour(kind), colour: nil)
}

/// The kind colour the core gives `kind` (plan 7b Ruling 10), a bus
/// taken as fed by an output.
func kindColour(_ kind: StripKindRow) -> StripColourRow {
    switch kind {
    case .builtinSynth, .instrument, .instrumentOutput: .instrument
    case .audioPath: .channel
    case .bus: .bus
    case .master: .master
    }
}

/// A mixer: Synth (built-in, one insert) and Bus 1, then Master.
func mixerFake() -> FakeSession {
    let f = demoFake()
    f.mixerSnap = MixerSnapshot(
        version: f.snap.version,
        strips: [
            stripRow(1, "Synth", kind: .builtinSynth, inserts: [slotRow(10, "Gain")]),
            stripRow(2, "Bus 1"),
            stripRow(0, "Master", kind: .master),
        ],
        buses: [BusRow(id: 2, name: "Bus 1")])
    return f
}

/// A 4/4, 120 BPM sequence info, 4 bars long.
func fourFour(_ seq: UInt64, name: String = "S") -> SequenceInfo {
    SequenceInfo(
        id: seq, name: name, comment: "", keyboardSlot: nil, startOffset: 0,
        lengthTicks: 4 * 61_440, tempo: [TempoPointRow(tick: 0, bpm: 120, ramp: false)],
        meter: [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)], markers: [],
        generated: false)
}

/// A note row.
func noteRow(_ id: UInt64, tick: Int64, pitch: UInt8 = 60, duration: Int64 = 15_360) -> EventRow {
    EventRow(
        id: id, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: pitch,
        velocity: 0x8000, releaseVelocity: 0, duration: duration, value: 0, value2: 0,
        attribute: nil, perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0,
        media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
        fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear),
        text: nil, textKind: nil, data: Data(), eightBit: false)
}

/// An audio region of media `media`: `frames` frames shown over
/// `duration` ticks.
func regionRow(
    _ id: UInt64, tick: Int64, duration: Int64, frames: UInt64, media: UInt64 = 7, gainDb: Float = 0,
    fadeIn: Int64 = 0, fadeOut: Int64 = 0
) -> EventRow {
    var r = noteRow(id, tick: tick, duration: duration)
    r.kind = .audioRegion
    r.media = media
    r.frames = frames
    r.gainDb = gainDb
    r.fadeIn = FadeRow(length: fadeIn, shape: .linear)
    r.fadeOut = FadeRow(length: fadeOut, shape: .linear)
    return r
}

/// A media-pool row.
func mediaRow(
    _ id: UInt64, _ name: String, frames: UInt64 = 96_000, uses: UInt32 = 0, offline: OfflineReasonRow? = nil,
    used: Float = 0
) -> MediaRow {
    MediaRow(
        id: id, name: name, path: "Media/\(name).caf", channels: 2, frames: frames, sampleRate: 48_000,
        origin: .imported, originalName: "\(name).wav", offline: offline, uses: uses, usedPercent: used)
}

/// An events block, or a reference block when `ref` is set.
func blockRow(
    _ start: Int64, _ end: Int64, events: [UInt64], ref: UInt64? = nil, label: String? = nil
) -> BlockRow {
    BlockRow(
        start: start, end: end, kind: ref == nil ? .events : .reference, events: events,
        label: label, refSeq: ref, muted: false)
}
