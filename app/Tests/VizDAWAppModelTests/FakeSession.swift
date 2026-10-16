import Foundation
import VizDAWCore

/// A scripted `SessionProtocol` for view-model tests.
final class FakeSession: SessionProtocol, @unchecked Sendable {
    var snap: DocSnapshot
    var laneTable: [UInt64: [LaneRow]] = [:]
    var state = TransportState(
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [])
    var playError: VizError?
    var transportError: VizError?
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
            redoLabel: nil, sequences: sequences, warnings: [])
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

    func redo() throws -> DocSnapshot { commandResult() }

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
    func undo() throws -> DocSnapshot { commandResult() }

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

    func mixer() throws -> MixerSnapshot { mixerSnap }
    func levels() throws -> MixerLevels {
        levelsCalls += 1
        return levelsValue
    }
    func beginEdit(label: String) throws { calls.append("begin \(label)") }
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
    func destinations() throws -> [DestRow] { dests }
    func setDestinationPlugin(dest: UInt64, plugin: PluginRefRow) throws -> DocSnapshot {
        try mixerCall("destPlugin \(dest) \(plugin.id)")
    }

    // MARK: Parameters

    var paramTable: [UInt64: [ParamRow]] = [:]
    /// Live sets, in order: (slot, param, value).
    var liveSets: [(UInt64, UInt64, Double)] = []
    var endedGestures: [UInt64] = []

    func params(slot: UInt64) throws -> [ParamRow] { paramTable[slot] ?? [] }
    func setParam(slot: UInt64, param: UInt64, value: Double) throws {
        liveSets.append((slot, param, value))
    }
    func commitParams(slot: UInt64, values: [ParamValue]) throws -> DocSnapshot {
        try mixerCall("commitParams \(slot) " + values.map { "\($0.id)=\($0.value)" }.joined(separator: ","))
    }
    func endPluginGestures(slot: UInt64) throws { endedGestures.append(slot) }
}

func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
    SeqRow(
        id: id, name: name, useCount: uses, topLevel: topLevel, lengthTicks: 61_440,
        laneCount: 1, comment: "")
}

func lane(_ id: UInt64, _ name: String, mute: Bool = false) -> LaneRow {
    LaneRow(
        id: id, name: name, destination: nil, midiEventCount: 0, regionCount: 0,
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false)
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
