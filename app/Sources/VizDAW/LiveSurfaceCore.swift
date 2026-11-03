import VizDAWAppModel
import VizDAWCore

/// The core's live surfaces (plan 5f-1 Task 9): each method is its FFI
/// free function. Only the app makes one; tests pass a fake (pre-flight
/// M25), so no test creates a CoreMIDI client or writes surfaces.json.
final class LiveSurfaceCore: SurfaceCore {
    func list() -> [SurfaceRow] { VizDAWCore.surfaceList() }
    func ports() -> SurfacePortsRow { VizDAWCore.surfacePorts() }
    func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32 {
        try VizDAWCore.surfaceAdd(kind: k, input: i, output: o, position: p)
    }
    func remove(_ id: UInt32) throws { try VizDAWCore.surfaceRemove(id: id) }
    func state() -> SurfaceStateRow { VizDAWCore.surfacesState() }
    func commandMaps() -> [CommandMapRow] { VizDAWCore.commandMaps() }
    func removeCommandMap(_ command: String) throws { try VizDAWCore.removeCommandMap(command: command) }
    func learnCommand(_ id: String) throws { try VizDAWCore.learnCommand(id: id) }
    func cancelLearn() { VizDAWCore.cancelLearn() }
    func learnState() -> LearnStateRow { VizDAWCore.learnState() }
}
