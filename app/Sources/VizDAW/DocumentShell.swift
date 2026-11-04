import AppKit
import SwiftUI
import SymbioticIcons
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// Where a `MixerView` shows (plan 7a Ruling 8): its strips read it to open
/// the maps sheet, popovers and rename field in their own placement.
struct MixerPlacementKey: EnvironmentKey {
    static let defaultValue: MixerPlacement = .window
}

/// True inside the document window, whose `DocumentShell` hosts the one
/// learn alert (Ruling 8): the views inside skip their own.
struct LearnAlertHostedKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    var mixerPlacement: MixerPlacement {
        get { self[MixerPlacementKey.self] }
        set { self[MixerPlacementKey.self] = newValue }
    }

    var learnAlertHosted: Bool {
        get { self[LearnAlertHostedKey.self] }
        set { self[LearnAlertHostedKey.self] = newValue }
    }
}

/// The document window's content in layout A (plan 7a Rulings 2, 3 and 9):
/// the transport bar, the banners, the left, centre, bottom and right
/// regions with their dividers, and the status line.
struct DocumentShell: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        VStack(spacing: 0) {
            DocumentTransportBar(model: model, docID: docID, showsViews: true)
            Divider()
            ProjectBanners(model: model)
            GeometryReader { g in
                regions(g.size)
            }
            StatusLine(editor: model.editor)
        }
        .environment(\.openPluginWindow, { slot in PluginWindows.show(appStore, doc: docID, slot: slot) })
        .environment(\.learnAlertHosted, true)
        .learnAlert(model.learn, shown: !model.learn.showsMaps)
        .frame(minWidth: 700, minHeight: 450)
    }

    @ViewBuilder
    private func regions(_ size: CGSize) -> some View {
        let views = model.editor.layout.views
        let f = PaneGeometry.frames(in: size, setup: views.current)
        let mix = views.main == .mix
        ZStack(alignment: .topLeading) {
            centre(mix: mix)
                .place(f.centre)
            if let r = f.left {
                PaneFrame(side: .left, model: model) { PaneHost(side: .left, model: model, docID: docID) }
                    .place(r)
            }
            // Always mounted, so the editor keeps its scroll and zoom while
            // the pane is hidden (Ruling 3): zero size, out of sight.
            let bottomHidden = f.bottom == nil
            PaneFrame(side: .bottom, model: model) {
                PaneHost(side: .bottom, model: model, docID: docID, hidden: bottomHidden)
            }
            .place(f.bottom ?? .zero)
            .opacity(bottomHidden ? 0 : 1)
            .allowsHitTesting(!bottomHidden)
            .accessibilityHidden(bottomHidden)
            if let r = f.right {
                PaneFrame(side: .right, model: model) { PaneHost(side: .right, model: model, docID: docID) }
                    .place(r)
            }
            ForEach(PaneSide.allCases, id: \.self) { side in
                PaneDivider(side: side, model: model, size: size)
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
    }

    /// The Arrange centre stays mounted under the Mix view (Ruling 9); the
    /// mixer is made only while the Mix view shows.
    private func centre(mix: Bool) -> some View {
        ZStack {
            ArrangementPane(model: model)
                .background(ResignWhenHidden(hidden: mix))
                .opacity(mix ? 0 : 1)
                .allowsHitTesting(!mix)
                .accessibilityHidden(mix)
            if mix {
                if let mixer = appStore.mixer(docID) {
                    MixerView(model: mixer, placement: .view)
                } else {
                    ContentUnavailableView {
                        Label { Text("Mixer Unavailable") } icon: { Icon(AppIcon.mixer.role, size: 32) }
                    } description: {
                        Text("The mixer of this document could not be read.")
                    }
                }
            }
        }
    }
}

extension View {
    /// Puts the view at `r` in a top-leading `ZStack`'s coordinates; what
    /// does not fit is cut at the right and bottom, never centred under a
    /// neighbouring pane.
    fileprivate func place(_ r: CGRect) -> some View {
        frame(width: max(r.width, 0), height: max(r.height, 0), alignment: .topLeading)
            .clipped()
            .position(x: r.midX, y: r.midY)
    }
}

/// What a pane's picker can choose: a view, or (the bottom pane) an editor kind.
private enum PaneChoice: Hashable {
    case view(PaneView)
    case kind(DetailEditorKind)

    var title: String {
        switch self {
        case .view(let v): v.title
        case .kind(let k): k.title
        }
    }
}

/// A pane's frame: its view picker over its content (Ruling 3). The bottom
/// pane lists the editor kinds, then Mixer (Arrange only).
struct PaneFrame<Content: View>: View {
    let side: PaneSide
    let model: DocumentModel
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Picker(side.title, selection: selection) {
                    ForEach(choices, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.menu)
                .labelsHidden()
                .fixedSize()
                .help(side.title)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            Divider()
            // What does not fit goes off the right and bottom, never above
            // the picker.
            content()
                .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity, alignment: .topLeading)
        }
        .background(.background)
    }

    private var choices: [PaneChoice] {
        let views = ViewLayouts.views(side, in: model.editor.layout.views.main)
        guard side == .bottom else { return views.map(PaneChoice.view) }
        return DetailEditorKind.allCases.map(PaneChoice.kind) + views.filter { $0 != .editor }.map(PaneChoice.view)
    }

    private var selection: Binding<PaneChoice> {
        let m = model
        let side = side
        return Binding(
            get: {
                let l = m.editor.layout
                let p = l.views.current[side]
                if side == .bottom, p.view == .editor { return .kind(l.detail?.kind ?? l.lastKind) }
                return .view(p.view)
            },
            set: { c in
                let refused: String?
                switch c {
                case .view(let v):
                    refused = ViewCommands.choose(side, v, kind: nil, shown: nil, size: nil, in: nil, m)
                case .kind(let k):
                    refused = ViewCommands.choose(side, .editor, kind: k, shown: nil, size: nil, in: nil, m)
                }
                if let refused { m.editor.status = refused }
            })
    }
}

/// What a pane shows (plan 7a Task 8): one host per `PaneView`. The bottom
/// pane's editor stays mounted under its other views (Ruling 3).
struct PaneHost: View {
    let side: PaneSide
    let model: DocumentModel
    let docID: UUID
    /// The pane is off screen (the bottom pane's editor stays mounted).
    var hidden = false

    var body: some View {
        let view = model.editor.layout.views.current[side].view
        if side == .bottom {
            let editorHidden = hidden || view != .editor
            ZStack {
                // One line per label: an editor's bar in a narrow pane
                // truncates rather than wrap and push the editor down.
                DetailArea(model: model, docID: docID)
                    .lineLimit(1)
                    .background(ResignWhenHidden(hidden: editorHidden))
                    .opacity(view == .editor ? 1 : 0)
                    .allowsHitTesting(view == .editor)
                    .accessibilityHidden(editorHidden)
                if view != .editor, !hidden {
                    content(view)
                }
            }
        } else {
            content(view)
        }
    }

    /// `.editor` is only ever the bottom pane's always-mounted `DetailArea`.
    @ViewBuilder
    private func content(_ view: PaneView) -> some View {
        switch view {
        case .strip: LeftStripPane(model: model, docID: docID)
        case .sequences: SequenceSidebar(model: model)
        case .inspector: InspectorPane(model: model, docID: docID)
        case .editor: DetailArea(model: model, docID: docID)
        case .mixer:
            if let mixer = appStore.mixer(docID) {
                MixerView(model: mixer, placement: .pane)
            } else {
                ContentUnavailableView {
                    Label { Text("Mixer Unavailable") } icon: { Icon(AppIcon.mixer.role, size: 32) }
                } description: {
                    Text("The mixer of this document could not be read.")
                }
            }
        case .controllers: ControllersPane(model: model)
        case .mediaPool: MediaPoolView(media: model.media, compact: true)
        case .plugins: PluginsPane(model: model, docID: docID)
        case .markers: MarkersPane(model: model)
        }
    }
}

/// The 1-point line on a shown pane's inner edge, dragged to resize it
/// (Ruling 2). While dragging only the line moves; the size is set once,
/// when the drag ends, so editors do not redraw at pointer rate.
struct PaneDivider: View {
    let side: PaneSide
    let model: DocumentModel
    /// The regions' area under the transport bar.
    let size: CGSize
    /// The size the pane showed at when the drag began (its displayed,
    /// perhaps shrunk, size: Task 2 review M3).
    /// (`State` spelled out: the Command Line Tools cannot expand `@State`.)
    var dragStart = State<Double?>(initialValue: nil)
    /// The size the drag would give, shown by the line until it ends.
    var live = State<Double?>(initialValue: nil)

    /// The hit area across the line.
    private static let grip: CGFloat = 7

    var body: some View {
        let setup = model.editor.layout.views.current
        let f = PaneGeometry.frames(in: size, setup: setup)
        if let r = line(f) {
            let dragging = live.wrappedValue != nil
            let shift = offset()
            Rectangle()
                .fill(dragging ? Color.accentColor : Color(nsColor: .separatorColor))
                .frame(width: side == .bottom ? r.width : (dragging ? 2 : 1),
                       height: side == .bottom ? (dragging ? 2 : 1) : r.height)
                .frame(width: side == .bottom ? r.width : Self.grip, height: side == .bottom ? Self.grip : r.height)
                .contentShape(Rectangle())
                .onHover { inside in
                    if inside {
                        (side == .bottom ? NSCursor.resizeUpDown : NSCursor.resizeLeftRight).push()
                    } else {
                        NSCursor.pop()
                    }
                }
                .gesture(
                    DragGesture(minimumDistance: 1, coordinateSpace: .global)
                        .onChanged { v in changed(v.translation, frames: f, setup: setup) }
                        .onEnded { _ in ended() }
                )
                .position(x: r.midX + shift.width, y: r.midY + shift.height)
                // VoiceOver can size the pane, as it could the split views'
                // splitters before 7a (final review n8).
                .accessibilityElement()
                .accessibilityLabel("\(side.title) Size")
                .accessibilityValue("\(Int(setup[side].size)) points")
                .accessibilityAdjustableAction { d in
                    let step: Double = d == .increment ? 20 : d == .decrement ? -20 : 0
                    guard step != 0 else { return }
                    _ = model.editor.setPane(side, view: nil, shown: nil, size: setup[side].size + step, in: nil)
                }
        }
    }

    /// The line's rect: the left pane's right edge, the bottom row's top
    /// edge (across the right pane too), the right pane's left edge.
    private func line(_ f: PaneFrames) -> CGRect? {
        let d = PaneGeometry.divider
        switch side {
        case .left:
            return f.left.map { CGRect(x: $0.maxX, y: 0, width: d, height: size.height) }
        case .bottom:
            return f.bottom.map { CGRect(x: $0.minX, y: $0.minY - d, width: size.width - $0.minX, height: d) }
        case .right:
            return f.right.map { CGRect(x: $0.minX - d, y: $0.minY, width: d, height: $0.height) }
        }
    }

    private func offset() -> CGSize {
        guard let start = dragStart.wrappedValue, let now = live.wrappedValue else { return .zero }
        let delta = CGFloat(now - start)
        switch side {
        case .left: return CGSize(width: delta, height: 0)
        case .right: return CGSize(width: -delta, height: 0)
        case .bottom: return CGSize(width: 0, height: -delta)
        }
    }

    private func changed(_ translation: CGSize, frames f: PaneFrames, setup: PaneSetup) {
        let start = dragStart.wrappedValue ?? f.size(side) ?? setup[side].size
        dragStart.wrappedValue = start
        // `translation` is cumulative from the drag's start, so each change
        // starts from `dragStart` (Ruling 2), with every pane at its shown size.
        var seeded = f.displayed(setup)
        seeded[side].size = start
        let delta = side == .bottom ? -translation.height : translation.width
        live.wrappedValue = PaneGeometry.drag(seeded, side: side, by: delta, in: size)
    }

    private func ended() {
        if let size = live.wrappedValue, size != dragStart.wrappedValue {
            _ = model.editor.setPane(side, view: nil, shown: nil, size: size, in: nil)
        }
        dragStart.wrappedValue = nil
        live.wrappedValue = nil
    }
}

/// Inside an always-mounted layer (the Arrange centre, the bottom pane's
/// editor): when `hidden` turns true, the window's first responder, if it
/// is a view in the layer, resigns (Rulings 3 and 9). A hidden piano roll
/// then takes no Delete, and a list cell or lane name ends its edit
/// through its own end path.
struct ResignWhenHidden: NSViewRepresentable {
    let hidden: Bool

    final class Marker: NSView {
        var layerHidden = false
        /// The layer's last shown rect, in window coordinates (a hidden
        /// bottom pane has zero size by the time it is asked).
        private(set) var shownRect: NSRect = .zero

        // Inert for the user: clicks and VoiceOver pass through it.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }

        override func setFrameSize(_ s: NSSize) {
            super.setFrameSize(s)
            remember()
        }

        override func setFrameOrigin(_ p: NSPoint) {
            super.setFrameOrigin(p)
            remember()
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            remember()
        }

        func remember() {
            guard window != nil, !layerHidden else { return }
            let r = convert(bounds, to: nil)
            if !r.isEmpty { shownRect = r }
        }

        /// The layer's rect now, or its last shown one.
        var layerRect: NSRect {
            let r = convert(bounds, to: nil)
            return r.isEmpty ? shownRect : r
        }
    }

    func makeNSView(context: Context) -> Marker {
        let m = Marker()
        m.layerHidden = hidden
        return m
    }

    func updateNSView(_ m: Marker, context: Context) {
        let was = m.layerHidden
        m.layerHidden = hidden
        if hidden, !was {
            LayerFocus.resign(m.window, inside: m.layerRect)
        } else if !hidden {
            m.remember()
        }
    }
}
