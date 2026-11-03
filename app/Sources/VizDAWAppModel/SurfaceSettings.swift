import Foundation
import Observation
import VizDAWCore

/// The Add Surface sheet's fields (the view keeps no state of its own).
public struct SurfaceDraft: Equatable, Sendable {
    public var kind: SurfaceKindRow
    public var input: String
    public var output: String
    /// The MCU XT-compatible unit's place in the bank group (0–3).
    public var position: Int

    public init(kind: SurfaceKindRow = .mcu, input: String = "", output: String = "", position: Int = 1) {
        self.kind = kind
        self.input = input
        self.output = output
        self.position = position
    }
}

/// The core's surfaces as Settings › Surfaces edits them (plan 5f-1
/// Rulings 9–11). Everything goes through `SurfaceCore`: the app passes
/// the live one, tests a fake (pre-flight M25).
@MainActor @Observable public final class SurfaceSettings {
    @ObservationIgnored private let core: any SurfaceCore
    public private(set) var rows: [SurfaceRow] = []
    public private(set) var ports = SurfacePortsRow(sources: [], destinations: [])
    public private(set) var commandMaps: [CommandMapRow] = []
    public var alert: AlertItem?
    /// The open Add Surface sheet.
    public var draft: SurfaceDraft?
    /// Why the core refused the sheet's unit (shown in the sheet, which
    /// stays open so the user can change it; review M4).
    public private(set) var draftProblem: String?
    /// What the open sheet says about its type (nil: nothing).
    public private(set) var draftNote: String?
    /// `state.get surfaces`' problem lines as of the last refresh (pre-flight I4).
    public private(set) var problems: [String] = []

    nonisolated public static let noKontrol = "No Kontrol S-Series MK3 “DAW” port is connected: pick its ports by hand."
    nonisolated public static let kontrolHelp = "Use the keyboard's “DAW” port for input and output. Its “Main” port stays a MIDI keyboard for your lanes."
    nonisolated public static let kontrolNotice = "Kontrol and Native Instruments are trademarks of Native Instruments GmbH. VizDAW is not affiliated with or endorsed by Native Instruments."

    /// Reads nothing yet: Settings refreshes when it opens, so making this
    /// at launch asks the core nothing before `surfacesLoad`.
    public init(core: any SurfaceCore) {
        self.core = core
    }

    /// Settings polls it at 2 Hz while open (connection states change on
    /// their own); unchanged values are not reassigned, so the list does
    /// not redraw on every poll.
    public func refresh() {
        let r = core.list()
        if r != rows { rows = r }
        let p = core.ports()
        if p != ports { ports = p }
        let c = core.commandMaps()
        if c != commandMaps { commandMaps = c }
        let q = core.state().problems
        if q != problems { problems = q }
    }

    /// The last line about surface `id`, without "surface {id}: " (Ruling 10).
    public func problem(for id: UInt32) -> String? {
        let prefix = "surface \(id): "
        return problems.last { $0.hasPrefix(prefix) }.map { String($0.dropFirst(prefix.count)) }
    }

    /// The lines that name no listed row (an unknown kind kept, the file's).
    public var otherProblems: [String] {
        let prefixes = Set(rows.map { "surface \($0.id): " })
        return problems.filter { line in !prefixes.contains { line.hasPrefix($0) } }
    }

    /// A Kontrol is configured: the list shows the notice (M11).
    public var showsKontrolNotice: Bool { rows.contains { $0.kind == .kontrolMk3 } }

    /// Sets the sheet's type; a Kontrol takes its DAW port when one is found (Ruling 10).
    public func setDraftKind(_ k: SurfaceKindRow) {
        guard var d = draft else { return }
        d.kind = k
        draftNote = nil
        if k == .kontrolMk3 {
            let found = core.kontrolPorts()
            if let i = found.sources.first, let o = found.destinations.first {
                d.input = i
                d.output = o
            } else {
                draftNote = Self.noKontrol
            }
        }
        draft = d
    }

    /// Adds a unit; a refusal is an alert. A HUI-compatible unit is a bank
    /// of its own, so it is added without a position.
    public func add(_ kind: SurfaceKindRow, input: String, output: String, position: UInt8?) {
        if let problem = tryAdd(kind, input: input, output: output, position: position) {
            alert = AlertItem(title: "Add Surface Failed", message: problem)
        }
    }

    /// The core's refusal, if any.
    private func tryAdd(_ kind: SurfaceKindRow, input: String, output: String, position: UInt8?) -> String? {
        defer { refresh() }
        do {
            // A HUI-compatible unit and a Kontrol are banks of their own.
            _ = try core.add(kind, input, output, kind == .hui || kind == .kontrolMk3 ? nil : position)
            return nil
        } catch {
            return AlertItem(error, title: "Add Surface Failed").message
        }
    }

    public func remove(_ id: UInt32) {
        do {
            try core.remove(id)
        } catch {
            alert = AlertItem(error, title: "Remove Surface Failed")
        }
        refresh()
    }

    /// Removes the MIDI controls learned to command `command`.
    public func removeCommandMap(_ command: String) {
        do {
            try core.removeCommandMap(command)
        } catch {
            alert = AlertItem(error, title: "Remove MIDI Learn Failed")
        }
        refresh()
    }

    /// Opens the Add sheet with the first ports picked.
    public func beginAdd() {
        refresh()
        draftProblem = nil
        draftNote = nil
        draft = SurfaceDraft(input: ports.sources.first ?? "", output: ports.destinations.first ?? "")
    }

    /// Adds what the sheet says and closes it; a refusal keeps it open
    /// with the reason in it (review M4).
    public func commitAdd() {
        guard let d = draft else { return }
        let position = d.kind == .mcuXt ? UInt8(clamping: d.position) : nil
        if let problem = tryAdd(d.kind, input: d.input, output: d.output, position: position) {
            draftProblem = problem
        } else {
            draftProblem = nil
            draft = nil
        }
    }

    /// Closes the sheet without adding.
    public func cancelAdd() {
        draftProblem = nil
        draftNote = nil
        draft = nil
    }

    /// "MCU-compatible", "MCU XT-compatible", "HUI-compatible".
    nonisolated public static func title(_ k: SurfaceKindRow) -> String {
        switch k {
        case .mcu: "MCU-compatible"
        case .mcuXt: "MCU XT-compatible"
        case .hui: "HUI-compatible"
        case .kontrolMk3: "Kontrol S-Series MK3 (DAW mode)"
        }
    }

    /// "Connected", "Connected · not answering", "Disconnected".
    nonisolated public static func status(_ r: SurfaceRow) -> String {
        guard r.connected else { return "Disconnected" }
        return r.online ? "Connected" : "Connected · not answering"
    }
}
