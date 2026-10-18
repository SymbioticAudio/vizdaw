import VizDAWCore

/// What a Drum menu command came to.
public enum DrumOutcome: Equatable, Sendable {
    case done
    /// The menu item is disabled: why.
    case unavailable(String)
    /// The core refused it: the status line's message.
    case refused(String)
}

/// The Drum menu (Plan 4e Ruling 13): one implementation for the menu
/// items and the control channel's `command.run`.
@MainActor
public enum DrumCommands {
    public static let markLane = "drum.mark_lane"
    public static let addRow = "drum.add_row"
    public static let showVelocity = "drum.show_velocity"
    public static let splitRow = "drum.split_row"
    public static let removeRow = "drum.remove_row"
    public static let timeline = "drum.timeline"

    public static let ids: Set<String> = [markLane, addRow, showVelocity, splitRow, removeRow, timeline]

    static let noLane = "Select a lane in the arrangement first."
    static let noGrid = "Open a drum grid first."

    /// Whether `id` can run on `m` now (its menu item is enabled).
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case markLane: m.selectedLane != nil
        case addRow: m.drumGridForCommands()?.currentLane != nil
        case showVelocity, splitRow, removeRow: m.drumGridForCommands()?.selected != nil
        case timeline: true
        default: false
        }
    }

    /// A toggle's state (its menu item's check mark).
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case markLane: m.selectedLane?.isDrum ?? false
        case showVelocity: m.drumGridForCommands()?.selected?.row.showVelocity ?? false
        case timeline: m.editor.layout.drumGrid.view == .timeline
        default: false
        }
    }

    /// Runs `id` on `m`; `on` sets a toggle (nil flips it).
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> DrumOutcome {
        if id == timeline {
            let want = on ?? !isOn(timeline, m)
            m.editor.updateLayout { $0.drumGrid.view = want ? .timeline : .matrix }
            return .done
        }
        if id == markLane {
            guard let seq = m.selectedSeq, let lane = m.selectedLane else { return .unavailable(noLane) }
            let want = on ?? !lane.isDrum
            guard want != lane.isDrum else { return .done }
            return m.editor.markDrumLane(seq: seq, lane: lane.id, on: want) ? .done : refused(m)
        }
        guard let g = m.drumGridForCommands() else { return .unavailable(noGrid) }
        if id == addRow {
            guard g.currentLane != nil else { return .unavailable("The drum grid shows no drum lane.") }
            return g.addRow() ? .done : refused(m)
        }
        guard g.selected != nil else { return .unavailable(DrumGridModel.noRowMessage) }
        let ok: Bool
        switch id {
        case showVelocity: ok = g.showVelocity(on)
        case splitRow: ok = g.splitSelectedRow()
        case removeRow: ok = g.removeSelectedRow()
        default: return .unavailable("VizDAW has no handler for \(id).")
        }
        return ok ? .done : refused(m)
    }

    private static func refused(_ m: DocumentModel) -> DrumOutcome {
        .refused(m.editor.status ?? "The drum command was refused.")
    }
}
