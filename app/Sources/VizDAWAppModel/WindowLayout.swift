import Foundation

/// The window's two main views (spec §6, Plan 7a Ruling 1).
public enum MainView: String, CaseIterable, Codable, Equatable, Sendable {
    case arrange, mix

    public var title: String {
        switch self {
        case .arrange: "Arrange"
        case .mix: "Mix"
        }
    }
}

/// The three panes around the centre.
public enum PaneSide: String, CaseIterable, Codable, Equatable, Sendable {
    case left, bottom, right

    public var title: String {
        switch self {
        case .left: "Left Pane"
        case .bottom: "Bottom Pane"
        case .right: "Right Pane"
        }
    }
}

/// What a pane shows. The raw values are the control channel's names.
public enum PaneView: String, CaseIterable, Codable, Equatable, Sendable {
    case strip, sequences, inspector, editor, mixer, controllers
    case mediaPool = "media_pool"
    case plugins, markers

    public var title: String {
        switch self {
        case .strip: "Channel Strip"
        case .sequences: "Sequences"
        case .inspector: "Inspector"
        case .editor: "Editor"
        case .mixer: "Mixer"
        case .controllers: "Controllers"
        case .mediaPool: "Media Pool"
        case .plugins: "Plug-ins"
        case .markers: "Markers"
        }
    }
}

/// Where the mixer shows (used by the window tasks).
public enum MixerPlacement: String, Sendable { case window, pane, view, strip }

public struct PaneState: Encodable, Equatable, Sendable {
    public var shown: Bool
    public var view: PaneView
    public var size: Double

    public init(shown: Bool, view: PaneView, size: Double) {
        self.shown = shown
        self.view = view
        self.size = size
    }
}

public struct PaneSetup: Encodable, Equatable, Sendable {
    public var left: PaneState
    public var bottom: PaneState
    public var right: PaneState

    public init(left: PaneState, bottom: PaneState, right: PaneState) {
        self.left = left
        self.bottom = bottom
        self.right = right
    }

    public subscript(side: PaneSide) -> PaneState {
        get {
            switch side {
            case .left: left
            case .bottom: bottom
            case .right: right
            }
        }
        set {
            switch side {
            case .left: left = newValue
            case .bottom: bottom = newValue
            case .right: right = newValue
            }
        }
    }
}

/// Decoding helpers: a field that is missing or malformed is nil alone.
private struct PanePatch: Decodable {
    var shown: Bool?
    var view: PaneView?
    var size: Double?

    enum CodingKeys: String, CodingKey { case shown, view, size }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        shown = c.field(.shown, nil)
        view = c.field(.view, nil)
        size = c.field(.size, nil)
    }

    func merged(onto d: PaneState) -> PaneState {
        PaneState(shown: shown ?? d.shown, view: view ?? d.view, size: size ?? d.size)
    }
}

private struct SetupPatch: Decodable {
    var left: PanePatch?
    var bottom: PanePatch?
    var right: PanePatch?

    enum CodingKeys: String, CodingKey { case left, bottom, right }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        left = c.field(.left, nil)
        bottom = c.field(.bottom, nil)
        right = c.field(.right, nil)
    }

    func merged(onto d: PaneSetup) -> PaneSetup {
        PaneSetup(
            left: left?.merged(onto: d.left) ?? d.left,
            bottom: bottom?.merged(onto: d.bottom) ?? d.bottom,
            right: right?.merged(onto: d.right) ?? d.right)
    }
}

/// Each main view's pane setup, saved in `ui.json` (Plan 7a Ruling 1).
public struct ViewLayouts: Codable, Equatable, Sendable {
    public var main: MainView
    public var arrange: PaneSetup
    public var mix: PaneSetup
    /// The document window's `[x, y, width, height]`.
    public var frame: [Double]?

    public init() {
        main = .arrange
        arrange = PaneSetup(
            left: PaneState(shown: true, view: .strip, size: 220),
            bottom: PaneState(shown: true, view: .editor, size: 260),
            right: PaneState(shown: false, view: .controllers, size: 260))
        mix = PaneSetup(
            left: PaneState(shown: false, view: .strip, size: 220),
            bottom: PaneState(shown: false, view: .editor, size: 260),
            right: PaneState(shown: false, view: .plugins, size: 260))
        frame = nil
    }

    enum CodingKeys: String, CodingKey { case main, arrange, mix, frame }

    public init(from decoder: Decoder) throws {
        self.init()
        guard let c = try? decoder.container(keyedBy: CodingKeys.self) else { return }
        let d = ViewLayouts()
        main = c.field(.main, d.main)
        let a: SetupPatch? = c.field(.arrange, nil)
        let m: SetupPatch? = c.field(.mix, nil)
        arrange = a?.merged(onto: d.arrange) ?? d.arrange
        mix = m?.merged(onto: d.mix) ?? d.mix
        frame = c.field(.frame, nil)
        self = sanitized()
    }

    public subscript(view: MainView) -> PaneSetup {
        get { view == .arrange ? arrange : mix }
        set {
            if view == .arrange { arrange = newValue } else { mix = newValue }
        }
    }

    public var current: PaneSetup {
        get { self[main] }
        set { self[main] = newValue }
    }

    /// The views a pane can show; the first is its default kind.
    public static func views(_ side: PaneSide, in main: MainView) -> [PaneView] {
        switch side {
        case .left: [.strip, .sequences, .inspector]
        case .bottom: main == .arrange ? [.editor, .mixer] : [.editor]
        case .right: [.controllers, .mediaPool, .plugins, .markers]
        }
    }

    public static let sizeRange: [PaneSide: ClosedRange<Double>] = [
        .left: 160...480, .bottom: 120...900, .right: 180...520,
    ]

    /// Repairs off-list views, bad sizes and a malformed frame.
    public func sanitized() -> ViewLayouts {
        var v = self
        let d = ViewLayouts()
        for main in MainView.allCases {
            for side in PaneSide.allCases {
                var p = v[main][side]
                let dp = d[main][side]
                if !Self.views(side, in: main).contains(p.view) { p.view = dp.view }
                if let r = Self.sizeRange[side] {
                    p.size = p.size.isFinite ? min(max(p.size, r.lowerBound), r.upperBound) : dp.size
                }
                v[main][side] = p
            }
        }
        if let f = v.frame, f.count != 4 || !f.allSatisfy(\.isFinite) { v.frame = nil }
        return v
    }
}

extension EditorModel {
    /// Shows `v`, keeping each view's own panes.
    public func showMainView(_ v: MainView) {
        updateLayout { $0.views.main = v }
    }

    /// Shows, hides (`on == false`) or toggles (nil) a pane; its view and size stay.
    public func setPaneShown(_ side: PaneSide, _ on: Bool?, in main: MainView? = nil) {
        updateLayout { l in
            let m = main ?? l.views.main
            l.views[m][side].shown = on ?? !l.views[m][side].shown
        }
    }

    /// Sets a pane's view, shown flag and size (clamped). Returns the refusal
    /// message, or nil when applied.
    public func setPane(
        _ side: PaneSide, view: PaneView?, shown: Bool?, size: Double?, in main: MainView? = nil
    ) -> String? {
        let m = main ?? layout.views.main
        if let view, !ViewLayouts.views(side, in: m).contains(view) {
            return "\(view.rawValue) is not on the \(side.rawValue) pane's list in the \(m.rawValue) view."
        }
        updateLayout { l in
            var p = l.views[m][side]
            if let view { p.view = view }
            if let shown { p.shown = shown }
            if let size, size.isFinite, let r = ViewLayouts.sizeRange[side] {
                p.size = min(max(size, r.lowerBound), r.upperBound)
            }
            l.views[m][side] = p
        }
        return nil
    }

    /// Remembers the document window's `[x, y, width, height]`.
    public func setDocumentFrame(_ f: [Double]) {
        // A move that ends where it began writes nothing (Ruling 11; final
        // review n2): no redraw of every editor for a click on the title bar.
        guard f.count == 4, f.allSatisfy(\.isFinite), f != layout.views.frame else { return }
        updateLayout { $0.views.frame = f }
    }
}
