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
