import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The per-note expression pane under the region strip (spec §4b, Plan 4b
/// Ruling 14): the edit lane's notes' values of one kind; any tool but the
/// eraser draws them, the eraser removes them; a stroke is one undo step.
@MainActor
public final class ExpressionPaneView: NSView, TimeAxisFollower {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    public var timeAxis = TimeAxis() { didSet { needsDisplay = true } }

    /// The stroke so far, in time-area coordinates.
    private var stroke: [CGPoint] = []
    private var erasing = false
    private var start: CGPoint?
    private var open = false

    var side: CGFloat { PianoRollContainer.sideWidth }
    var kind: ExpressionKind { editor.layout.pianoRoll.expression }
    var values: ValueAxis { ValueAxis(height: bounds.height) }

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
            _ = model.chosenLane
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    public func wantedHeight() -> CGFloat {
        editor.layout.pianoRoll.showExpression ? ExpressionGeometry.paneHeight : 0
    }

    func editNotes() -> [EventRow] { model.editLane.map { model.notes($0) } ?? [] }

    // MARK: Drawing

    public override func draw(_ dirty: NSRect) {
        NSColor.textBackgroundColor.setFill()
        dirty.fill()
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: side, height: bounds.height).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
        NSString(string: ExpressionGeometry.shortTitle(kind) + " ▾").draw(
            at: CGPoint(x: 6, y: 6),
            withAttributes: [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.labelColor])
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height)).addClip()
        let v = values
        if kind == .pitchBend {
            NSColor.separatorColor.setFill()
            NSRect(x: side, y: v.y(0x8000_0000), width: bounds.width, height: 0.5).fill()
        }
        guard let lane = model.editLane else {
            NSGraphicsContext.restoreGraphicsState()
            return
        }
        let color = PianoRollPalette.color(model.colorIndex(lane))
        let from = timeAxis.tick(atX: dirty.minX - side)
        let to = timeAxis.tick(atX: dirty.maxX - side)
        for n in editNotes() where n.tick <= to && n.tick + n.duration >= from {
            let selected = editor.isSelected(event: n.id)
            let x0 = side + timeAxis.x(n.tick)
            let x1 = side + timeAxis.x(n.tick + n.duration)
            color.withAlphaComponent(0.08).setFill()
            NSRect(x: x0, y: 0, width: max(x1 - x0, 1), height: bounds.height).fill()
            let path = NSBezierPath()
            for (i, s) in ExpressionGeometry.steps(n, kind: kind.row, neutral: kind.neutral).enumerated() {
                let a = CGPoint(x: side + timeAxis.x(s.from), y: v.y(s.value))
                let b = CGPoint(x: side + timeAxis.x(s.to), y: v.y(s.value))
                if i == 0 { path.move(to: a) } else { path.line(to: a) }
                path.line(to: b)
            }
            (selected ? NSColor.controlAccentColor : color).setStroke()
            path.lineWidth = selected ? 2 : 1.5
            path.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    // MARK: Strokes (view coordinates)

    func strokeBegan(at p: CGPoint) {
        stroke = []
        open = false
        start = nil
        guard p.x >= side else { return }
        erasing = editor.tool == .eraser
        start = p
        stroke = [CGPoint(x: p.x - side, y: p.y)]
    }

    func strokeMoved(to p: CGPoint) {
        guard let s = start else { return }
        stroke.append(CGPoint(x: max(p.x, side) - side, y: p.y))
        if !open {
            guard hypot(p.x - s.x, p.y - s.y) > ToolMachine<Int>.deadZone, let lane = model.editLane else { return }
            open = editor.beginGesture(label, lanes: [lane])
        }
        if open { editor.updateGesture(command()) }
    }

    func strokeEnded() {
        defer {
            stroke = []
            start = nil
            open = false
        }
        guard start != nil else { return }
        if open {
            editor.endGesture(command())
        } else if let c = command(), let lane = model.editLane {
            // A tap is one step named like a stroke.
            let pending = editor.gesture != nil
            guard editor.beginGesture(label, lanes: [lane]) else {
                if pending { editor.status = EditorModel.gestureOpenMessage }
                return
            }
            editor.endGesture(c)
        }
    }

    private var label: String { erasing ? "Erase Expression" : "Draw Expression" }

    /// Leaving the window ends the stroke: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        strokeCancelled()
    }

    func strokeCancelled() {
        if open { editor.cancelGesture() }
        stroke = []
        start = nil
        open = false
    }

    private func command() -> EditCommand? {
        ExpressionGeometry.command(
            stroke: stroke, erase: erasing, kind: kind.row, notes: editNotes(),
            selected: Set(editor.selection.events), time: timeAxis, values: values)
    }

    // MARK: Events

    public override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = convert(e.locationInWindow, from: nil)
        if p.x < side {
            kindMenu().popUp(positioning: nil, at: p, in: self)
            return
        }
        strokeBegan(at: p)
    }

    public override func mouseDragged(with e: NSEvent) { strokeMoved(to: convert(e.locationInWindow, from: nil)) }
    public override func mouseUp(with e: NSEvent) { strokeEnded() }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 { strokeCancelled() } else { super.keyDown(with: e) }
    }

    func kindMenu() -> NSMenu {
        let menu = NSMenu()
        for k in ExpressionKind.choices {
            menu.addItem(
                MenuAction(title: k.title) { [weak self] in
                    self?.editor.updateLayout { $0.pianoRoll.expression = k }
                }.item(checked: k == kind))
        }
        return menu
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Expression: \(kind.title)" }
}
