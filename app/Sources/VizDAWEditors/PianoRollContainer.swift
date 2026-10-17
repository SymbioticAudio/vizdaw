import AppKit
import Observation
import VizDAWAppModel

/// A view under the notes that shares their time axis.
@MainActor
public protocol TimeAxisFollower: AnyObject {
    var timeAxis: TimeAxis { get set }
}

/// The piano roll's views (Plan 4b Ruling 12): a 64-pt side column (the
/// keyboard, or blank) beside the notes, and below them the lower panes
/// (region strip, expression pane), whose own side columns hold their
/// headers, so every pane shares one time axis and x origin.
@MainActor
public final class PianoRollContainer: NSView {
    public static let sideWidth: CGFloat = 64

    public let model: PianoRollModel
    public let canvas: PianoRollView
    public let keyboard: PianoKeyboardView
    private var lower: [(view: NSView & TimeAxisFollower, height: @MainActor () -> CGFloat)] = []

    public init(model: PianoRollModel) {
        self.model = model
        canvas = PianoRollView(model: model)
        keyboard = PianoKeyboardView(model: model)
        super.init(frame: .zero)
        addSubview(keyboard)
        addSubview(canvas)
        keyboard.pitchAxis = canvas.pitchAxis
        canvas.onPitchScroll = { [weak self] a in self?.keyboard.pitchAxis = a }
        canvas.onTimeAxis = { [weak self] a in
            self?.lower.forEach { $0.view.timeAxis = a }
        }
        // Lower panes (plan 4b Tasks 11 and 12) are added here.
        let strip = RegionStripView(model: model)
        addLowerPane(strip) { [weak strip] in strip?.wantedHeight() ?? 0 }
        let expression = ExpressionPaneView(model: model)
        addLowerPane(expression) { [weak expression] in expression?.wantedHeight() ?? 0 }
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// Adds a pane under the notes; `height` is asked on every layout (0
    /// hides it).
    public func addLowerPane(_ view: NSView & TimeAxisFollower, height: @escaping @MainActor () -> CGFloat) {
        lower.append((view, height))
        addSubview(view)
        view.timeAxis = canvas.axis
        needsLayout = true
    }

    private func observe() {
        withObservationTracking {
            _ = model.editor.layout.pianoRoll
            _ = model.editor.drawVersion
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsLayout = true
                self?.observe()
            }
        }
    }

    public override func layout() {
        super.layout()
        var bottom = bounds.height
        for pane in lower.reversed() {
            let h = min(max(pane.height(), 0), max(bottom - 80, 0))
            bottom -= h
            pane.view.isHidden = h == 0
            pane.view.frame = NSRect(x: 0, y: bottom, width: bounds.width, height: h)
        }
        let side = Self.sideWidth
        keyboard.isHidden = !model.editor.layout.pianoRoll.showKeyboard
        keyboard.frame = NSRect(x: 0, y: 0, width: side, height: max(bottom, 0))
        canvas.frame = NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: max(bottom, 0))
    }

    public override func scrollWheel(with e: NSEvent) { canvas.scrollWheel(with: e) }
}
