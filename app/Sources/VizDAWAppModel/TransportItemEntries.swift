import Foundation
import VizDAWCore

/// What each transport item does, apart from its view (groups-vcas-transport
/// Rulings 13 and 14; pre-flight F9: the app target's views stay thin, and
/// tests reach this): its labels, its own menu, and its entry in ».
@MainActor
public enum TransportItemEntries {
    /// Click's levels in dB (Ruling 13), loudest first.
    public static let clickLevels: [Double] = [0, -3, -6, -12, -18, -24, -36, -48, -60]

    /// A level as the Level menu shows it: "0 dB", "−12 dB".
    public static func levelTitle(_ db: Double) -> String {
        db < 0 ? "−\(Int(-db)) dB" : "\(Int(db)) dB"
    }

    /// A level as `transport.set_click_level` takes it: "-12".
    public static func levelArgument(_ db: Double) -> String { "\(Int(db))" }

    /// The Mode menu's titles.
    public static func modeTitle(_ mode: ClickModeRow) -> String {
        switch mode {
        case .playAndRecord: "Play and Record"
        case .recordOnly: "Record Only"
        }
    }

    static func modeArgument(_ mode: ClickModeRow) -> String {
        switch mode {
        case .playAndRecord: "play_and_record"
        case .recordOnly: "record_only"
        }
    }

    /// Whether the metronome is on (off while the session has not said).
    public static func clickIsOn(_ m: DocumentModel) -> Bool { m.transport.click?.on ?? false }

    /// The Click item's toggle: `transport.click`, as the menu item runs it.
    public static func toggleClick(_ m: DocumentModel) {
        report(TransportCommands.run(TransportCommands.click, m), m)
    }

    /// Click's own menu (its right-click and its ⌄): Level › and Mode ›,
    /// the current ones checked. Disabled while the session has not given
    /// the settings.
    public static func clickMenu(_ m: DocumentModel) -> [TransportMenuEntry] {
        let c = m.transport.click
        let levels = clickLevels.map { db in
            TransportMenuEntry.action(
                title: levelTitle(db), isOn: c.map { abs($0.levelDb - db) < 0.05 } ?? false, enabled: c != nil
            ) {
                let arg = ControlArg(name: "level_db", value: .text(value: levelArgument(db)))
                report(TransportCommands.runClick(TransportCommands.clickLevel, args: [arg], m), m)
            }
        }
        let modes = [ClickModeRow.playAndRecord, .recordOnly].map { mode in
            TransportMenuEntry.action(title: modeTitle(mode), isOn: c?.mode == mode, enabled: c != nil) {
                let arg = ControlArg(name: "mode", value: .text(value: modeArgument(mode)))
                report(TransportCommands.runClick(TransportCommands.clickMode, args: [arg], m), m)
            }
        }
        return [
            .submenu(title: "Level", isOn: nil, enabled: c != nil, entries: levels),
            .submenu(title: "Mode", isOn: nil, enabled: c != nil, entries: modes),
        ]
    }

    /// Click in »: the toggle with its check, then Level › and Mode ›.
    public static func clickEntry(_ m: DocumentModel) -> TransportMenuEntry {
        let on = clickIsOn(m)
        return .submenu(
            title: TransportItemID.click.title, isOn: on, enabled: true,
            entries: [
                .action(title: TransportItemID.click.title, isOn: on, enabled: m.transport.click != nil) {
                    toggleClick(m)
                }
            ] + clickMenu(m))
    }

    /// The pane toggles' ids, sides and keys, in bar order.
    public static let panes: [(id: String, side: PaneSide, key: String)] = [
        ("ui.toggle_left", .left, "⌃⌘←"), ("ui.toggle_bottom", .bottom, "⌃⌘↓"), ("ui.toggle_right", .right, "⌃⌘→"),
    ]

    /// The DSP load as the item shows it: "DSP 12%".
    public static func dspText(_ load: Float) -> String {
        let pct = load.isFinite ? max(min(load, 99.99), 0) * 100 : 0
        return String(format: "DSP %.0f%%", pct)
    }

    /// `item`'s entry in » (Ruling 14); nil for what never folds (the
    /// cluster, spaces, separators) and for a document-only item outside
    /// the document window.
    public static func overflow(
        _ item: TransportItem, _ m: DocumentModel, documentWindow: Bool, dspLoad: Float = 0
    ) -> TransportMenuEntry? {
        let id = item.id
        if id.documentOnly && !documentWindow { return nil }
        let title = id.title
        switch id {
        case .cluster, .space, .separator:
            return nil
        case .loop:
            return .action(title: title, isOn: m.looping, enabled: true) { m.toggleLoop() }
        case .click:
            return clickEntry(m)
        case .replaceOverdub:
            return .action(title: title, isOn: !m.record.replaces, enabled: !m.record.isRecording) {
                m.record.pressMode()
            }
        case .countIn:
            return .action(title: title, isOn: m.record.countInBars > 0, enabled: !m.record.isRecording) {
                m.record.toggleCountIn()
            }
        case .recordOptions, .positionField, .editFields, .punchFields, .mainView:
            return .popover(title: title)
        case .summary:
            return .text("\(title): \(m.stepEntry.summary ?? "off")")
        case .bar, .time, .tempo, .signature:
            return .text("\(title): \(m.readoutTexts.text(box(id) ?? .bar))")
        case .dspLoad:
            return .text("\(title): \(dspText(dspLoad))")
        case .device:
            return .text("\(title): \(Formatting.device(m.transport))")
        case .faults:
            return .action(title: title, isOn: nil, enabled: m.hasFaults || m.hasReport) { m.showsFaults.toggle() }
        case .panes:
            return .submenu(
                title: title, isOn: nil, enabled: true,
                entries: panes.map { p in
                    .action(title: p.side.title, isOn: ViewCommands.isOn(p.id, m), enabled: true) {
                        _ = ViewCommands.run(p.id, on: nil, m)
                    }
                })
        }
    }

    /// The readout box an item shows.
    public static func box(_ id: TransportItemID) -> TransportReadouts.Box? {
        switch id {
        case .bar: .bar
        case .time: .time
        case .tempo: .tempo
        case .signature: .signature
        default: nil
        }
    }

    /// The transport fields an item shows.
    public static func fields(_ id: TransportItemID) -> [TransportField] {
        switch id {
        case .positionField: [.position]
        case .editFields: [.editIn, .editOut]
        case .punchFields: [.punchIn, .punchOut]
        default: []
        }
    }

    /// A refusal goes to the status line, as a menu item's does.
    static func report(_ o: KeyOutcome, _ m: DocumentModel) {
        switch o {
        case .refused(let why), .busy(let why): m.editor.status = why
        case .done, .unavailable: break
        }
    }
}
