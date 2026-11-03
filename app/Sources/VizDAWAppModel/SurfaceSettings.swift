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
    }

    /// Adds a unit; a refusal is an alert. A HUI-compatible unit is a bank
    /// of its own, so it is added without a position.
    public func add(_ kind: SurfaceKindRow, input: String, output: String, position: UInt8?) {
        do {
            _ = try core.add(kind, input, output, kind == .hui ? nil : position)
        } catch {
            alert = AlertItem(error, title: "Add Surface Failed")
        }
        refresh()
    }

    public func remove(_ id: UInt32) {
        do {
            try core.remove(id)
        } catch {
            alert = AlertItem(error, title: "Remove Surface Failed")
        }
        refresh()
    }

    /// Opens the Add sheet with the first ports picked.
    public func beginAdd() {
        refresh()
        draft = SurfaceDraft(input: ports.sources.first ?? "", output: ports.destinations.first ?? "")
    }

    /// Adds what the sheet says and closes it.
    public func commitAdd() {
        guard let d = draft else { return }
        draft = nil
        add(d.kind, input: d.input, output: d.output, position: d.kind == .mcuXt ? UInt8(clamping: d.position) : nil)
    }

    /// "MCU-compatible", "MCU XT-compatible", "HUI-compatible".
    nonisolated public static func title(_ k: SurfaceKindRow) -> String {
        switch k {
        case .mcu: "MCU-compatible"
        case .mcuXt: "MCU XT-compatible"
        case .hui: "HUI-compatible"
        }
    }

    /// "Connected", "Connected · not answering", "Disconnected".
    nonisolated public static func status(_ r: SurfaceRow) -> String {
        guard r.connected else { return "Disconnected" }
        return r.online ? "Connected" : "Connected · not answering"
    }
}
