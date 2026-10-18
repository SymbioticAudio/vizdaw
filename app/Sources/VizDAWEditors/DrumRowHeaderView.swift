import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The drum grid's row headers (spec §4e): each lane's name and
/// destination, and per row its name, note and (when it has one of its
/// own) destination. A click selects a row, a double-click
/// renames it, a drag moves it (to another place, another drum lane, or
/// below the last lane for a lane of its own), the context menu holds the
/// row commands.
@MainActor
public final class DrumRowHeaderView: NSView, NSTextFieldDelegate {
    public static let width: CGFloat = 150

    public let model: DrumGridModel
    var editor: EditorModel { model.editor }
    public var verticalOffset: CGFloat = 0 { didSet { needsDisplay = true } }
    /// The row being dragged and the pointer's height (view coordinates).
    private(set) var dragging: (press: DrumPress, y: CGFloat, moved: Bool)?
    /// The row being renamed.
    private(set) var renaming: (key: DrumRowKey, field: NSTextField)?
    private var a11yElements: [CanvasElement] = []

    public init(model: DrumGridModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    /// Takes the keyboard on a press so Escape reaches a row drag.
    public override var acceptsFirstResponder: Bool { true }

    /// A click the control channel sends is never a window-activating first
    /// click (control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = model.selectedRow
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    /// Content height `y` (the canvas's coordinates below its ruler) of a
    /// point in this view.
    func contentY(_ viewY: CGFloat) -> Double { Double(viewY - EditorCanvas.rulerHeight + verticalOffset) }

    /// A band's rect in this view.
    func rect(_ b: DrumBandRow) -> CGRect {
        CGRect(
            x: 0, y: EditorCanvas.rulerHeight + CGFloat(b.top) - verticalOffset, width: bounds.width,
            height: CGFloat(b.height))
    }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let lanes = model.lanes
        for b in model.layout.bands {
            let r = rect(b)
            guard r.maxY >= dirty.minY, r.minY <= dirty.maxY else { continue }
            switch b.kind {
            case let .header(lane):
                guard lanes.indices.contains(Int(lane)) else { continue }
                let l = lanes[Int(lane)]
                NSString(string: "\(l.name) · \(l.destination ?? "No Destination")").draw(
                    in: r.insetBy(dx: 6, dy: 3),
                    withAttributes: [.font: NSFont.systemFont(ofSize: 12, weight: .semibold)])
            case let .row(lane, row):
                guard let d = model.row(Int(lane), Int(row)) else { continue }
                if model.selectedRow == DrumRowKey(lane: lanes[Int(lane)].lane, row: d.id) {
                    NSColor.controlAccentColor.withAlphaComponent(0.2).setFill()
                    r.fill()
                }
                // Every row names its destination; an inherited one (the
                // lane's) is dimmed (Ruling F2).
                let cap = Self.caption(d)
                let text = NSMutableAttributedString(
                    string: cap.name, attributes: [.font: NSFont.systemFont(ofSize: 11)])
                if let dest = cap.destination {
                    text.append(
                        NSAttributedString(
                            string: " · \(dest)",
                            attributes: [
                                .font: NSFont.systemFont(ofSize: 11),
                                .foregroundColor: cap.inherited
                                    ? NSColor.tertiaryLabelColor : NSColor.secondaryLabelColor,
                            ]))
                }
                text.draw(in: CGRect(x: 10, y: r.minY + 3, width: r.width - 50, height: 16))
                NSString(string: PitchName.name(d.note)).draw(
                    in: CGRect(x: r.width - 38, y: r.minY + 3, width: 34, height: 16),
                    withAttributes: [
                        .font: NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular),
                        .foregroundColor: NSColor.secondaryLabelColor,
                    ])
            case .newLane:
                continue
            }
            NSColor.separatorColor.setFill()
            CGRect(x: 0, y: r.maxY - 0.5, width: bounds.width, height: 1).fill()
        }
        if let d = dragging, d.moved {
            NSColor.controlAccentColor.setFill()
            CGRect(x: 0, y: d.y - 1, width: bounds.width, height: 2).fill()
        }
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: EditorCanvas.rulerHeight).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    /// A row's header text: its name and the destination its notes play on;
    /// `inherited` when that is the lane's, not the row's own.
    static func caption(_ d: DrumRowRow) -> (name: String, destination: String?, inherited: Bool) {
        (d.name, d.destination, d.dest == nil)
    }

    // MARK: Mouse

    private func press(at viewY: CGFloat) -> DrumPress? {
        guard case let .row(lane, row, _) = model.spot(contentY(viewY)) else { return nil }
        return DrumPress(lane: Int(lane), row: Int(row))
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        mouseDown(at: p, clicks: e.clickCount)
    }

    /// A press at `p` (view coordinates); tests call it directly.
    func mouseDown(at p: CGPoint, clicks: Int) {
        model.becameActive()
        guard let press = press(at: p.y) else { return }
        model.select(lane: press.lane, row: press.row)
        if clicks >= 2 {
            beginRename(press)
        } else {
            dragging = (press, p.y, false)
            window?.makeFirstResponder(self)
        }
    }

    public override func mouseDragged(with e: NSEvent) {
        mouseDragged(to: convert(e.locationInWindow, from: nil))
    }

    func mouseDragged(to p: CGPoint) {
        guard var d = dragging else { return }
        d.moved = d.moved || abs(p.y - d.y) > ToolMachine<Int>.deadZone
        d.y = p.y
        dragging = d
        needsDisplay = true
    }

    public override func mouseUp(with e: NSEvent) {
        mouseUp(at: convert(e.locationInWindow, from: nil))
    }

    func mouseUp(at p: CGPoint) {
        guard let d = dragging else { return }
        dragging = nil
        needsDisplay = true
        guard d.moved else { return }
        model.drop(lane: d.press.lane, row: d.press.row, atY: contentY(p.y))
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53, dragging != nil {  // Escape
            cancelOperation(nil)
        } else {
            super.keyDown(with: e)
        }
    }

    /// Escape: the drag ends and its release drops nothing.
    public override func cancelOperation(_ sender: Any?) {
        guard dragging != nil else { return }
        dragging = nil
        needsDisplay = true
    }

    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        guard let press = press(at: p.y) else { return nil }
        model.select(lane: press.lane, row: press.row)
        return rowMenu(press)
    }

    /// Rename, Show Velocity, Split to Own Lane, Remove Row.
    func rowMenu(_ press: DrumPress) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let shown = model.row(press.lane, press.row)?.showVelocity ?? false
        menu.addItem(MenuAction(title: "Rename…") { [weak self] in self?.beginRename(press) }.item(checked: false))
        menu.addItem(
            MenuAction(title: "Show Velocity") { [weak self] in self?.model.showVelocity(!shown) }.item(checked: shown))
        menu.addItem(.separator())
        menu.addItem(
            MenuAction(title: "Split Row to Own Lane") { [weak self] in self?.model.splitSelectedRow() }.item(
                checked: false))
        menu.addItem(
            MenuAction(title: "Remove Row") { [weak self] in self?.model.removeSelectedRow() }.item(checked: false))
        return menu
    }

    // MARK: Renaming

    func beginRename(_ press: DrumPress) {
        renaming?.field.removeFromSuperview()
        guard let r = model.row(press.lane, press.row),
            let b = model.layout.bands.first(where: { $0.kind == .row(lane: UInt32(press.lane), row: UInt32(press.row)) })
        else { return }
        let field = NSTextField(string: r.name)
        let band = rect(b)
        field.frame = CGRect(x: 8, y: band.minY + 1, width: band.width - 50, height: 20)
        field.delegate = self
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (DrumRowKey(lane: model.lanes[press.lane].lane, row: r.id), field)
        editor.beginTextEditing("drumRow")
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("drumRow")
        r.field.removeFromSuperview()
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        guard movement != .cancel else { return }
        model.rename(r.key, to: r.field.stringValue)
    }

    /// Escape cancels a rename: the name stays and no edit is sent.
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        cancelRename()
        return true
    }

    /// Ends a rename without renaming.
    func cancelRename() {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("drumRow")
        r.field.abortEditing()
        r.field.removeFromSuperview()
        window?.makeFirstResponder(self)
    }

    /// Torn down mid-rename: give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { dragging = nil }
        if window == nil, renaming != nil {
            renaming = nil
            editor.endTextEditing("drumRow")
        }
    }

    public override func scrollWheel(with e: NSEvent) {
        nextResponder?.scrollWheel(with: e)
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Drum Rows" }

    public override func accessibilityChildren() -> [Any]? {
        let lanes = model.lanes
        var items: [(item: CanvasAccessibilityItem, frame: CGRect)] = []
        for b in model.layout.bands {
            guard case let .row(lane, row) = b.kind, let d = model.row(Int(lane), Int(row)) else { continue }
            let r = rect(b)
            guard r.maxY > EditorCanvas.rulerHeight, r.minY < bounds.height else { continue }
            let press = DrumPress(lane: Int(lane), row: Int(row))
            let selected = model.selectedRow == DrumRowKey(lane: lanes[Int(lane)].lane, row: d.id)
            let label =
                "\(d.name), \(PitchName.name(d.note)), \(lanes[Int(lane)].name), \(d.destination ?? "no destination")"
            items.append(
                (
                    CanvasAccessibilityItem(
                        label: label, frame: r, selected: selected,
                        press: { [weak self] in self?.model.select(lane: press.lane, row: press.row) }), r
                ))
        }
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}

/// The drum grid's views: the row headers beside the hits, sharing one
/// vertical scroll.
@MainActor
public final class DrumGridContainer: NSView {
    public let model: DrumGridModel
    public let canvas: DrumGridView
    public let headers: DrumRowHeaderView

    public init(model: DrumGridModel) {
        self.model = model
        canvas = DrumGridView(model: model)
        headers = DrumRowHeaderView(model: model)
        super.init(frame: .zero)
        addSubview(headers)
        addSubview(canvas)
        canvas.onVerticalScroll = { [weak self] y in self?.headers.verticalOffset = y }
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        let w = DrumRowHeaderView.width
        headers.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        canvas.frame = NSRect(x: w, y: 0, width: max(bounds.width - w, 0), height: bounds.height)
    }

    public override func scrollWheel(with e: NSEvent) { canvas.scrollWheel(with: e) }
}
