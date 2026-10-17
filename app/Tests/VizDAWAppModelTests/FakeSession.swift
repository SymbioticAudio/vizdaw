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

    func locate(tick: Int64) throws { state.tick = tick }
    func path() throws -> String? { savedPath }

    func play(seq: UInt64) throws {
        if let e = playError { throw e }
        state.playing = true
        state.seq = seq
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

    func saveAs(path: String) throws {
        savedPath = path
        snap.dirty = false
        snap.title = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
    }

    func setLaneMute(seq: UInt64, lane: UInt64, mute: Bool) throws -> DocSnapshot {
        mutes.append((seq, lane, mute))
        return commandResult()
    }

    func setLoop(on: Bool) throws {}
    func snapshot() throws -> DocSnapshot { snap }
    func stop() throws { state.playing = false }
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

    var infoTable: [UInt64: SequenceInfo] = [:]
    var infoCalls = 0
    func modifyTemplates() throws -> [ModifyTemplateRow] { [] }
    func secondsAt(seq: UInt64, ticks: [Int64]) throws -> [Double] { ticks.map { Double($0) / 30_720 } }
    func ticksAt(seq: UInt64, seconds: [Double]) throws -> [Int64] { seconds.map { Int64(($0 * 30_720).rounded()) } }
    func lastModify() throws -> ModifyRow? { nil }

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
    func edit(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editError { throw e }
        edits.append(cmd)
        calls.append("edit")
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

    func abortEdit() throws -> DocSnapshot {
        calls.append("abort")
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
        activeTake: "Take 1", isDrum: false, arm: false, midiDest: nil, audioOut: nil)
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
        output: output, freeOutputs: [])
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

/// An events block, or a reference block when `ref` is set.
func blockRow(
    _ start: Int64, _ end: Int64, events: [UInt64], ref: UInt64? = nil, label: String? = nil
) -> BlockRow {
    BlockRow(
        start: start, end: end, kind: ref == nil ? .events : .reference, events: events,
        label: label, refSeq: ref, muted: false)
}
