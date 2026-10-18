import AppKit
import VizDAWAppModel
import VizDAWCore

public enum KeyboardLabels {
    /// Only C keys are labelled (Plan 4b Ruling 12).
    public static func label(_ p: UInt8) -> String? { p % 12 == 0 ? PitchName.name(p) : nil }
}

/// The optional keyboard beside the notes (spec §4b): one row per pitch,
/// aligned with the note rows; a click auditions through the edit lane's
/// destination, a drag plays each key it crosses (Plan 4b Ruling 6).
@MainActor
public final class PianoKeyboardView: NSView {
    public let model: PianoRollModel
    public var pitchAxis = PitchAxis() { didSet { needsDisplay = true } }
    private(set) var pressed: UInt8?

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    /// The key under `y`; the ruler-high strip on top is not a key.
    func pitch(atY y: CGFloat) -> UInt8? {
        y < EditorCanvas.rulerHeight ? nil : pitchAxis.pitch(atY: y - EditorCanvas.rulerHeight)
    }

    func press(atY y: CGFloat) {
        guard let p = pitch(atY: y) else { return }
        pressed = p
        play(p)
    }

    func dragTo(y: CGFloat) {
        guard pressed != nil, let p = pitch(atY: y), p != pressed else { return }
        pressed = p
        play(p)
    }

    func release() {
        pressed = nil
        model.silence()
    }

    private func play(_ p: UInt8) {
        model.sound(p, velocity: Pencil.velocity, dest: model.editLane.flatMap { model.row($0)?.midiDest })
    }

    public override func mouseDown(with e: NSEvent) { press(atY: convert(e.locationInWindow, from: nil).y) }
    public override func mouseDragged(with e: NSEvent) { dragTo(y: convert(e.locationInWindow, from: nil).y) }
    public override func mouseUp(with e: NSEvent) { release() }

    /// Leaving the window ends the press: its mouse-up will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        if newWindow == nil { release() }
    }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let top = EditorCanvas.rulerHeight
        let font = NSFont.systemFont(ofSize: 9)
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: NSRect(x: 0, y: top, width: bounds.width, height: max(bounds.height - top, 0))).addClip()
        for p in pitchAxis.visible(height: max(bounds.height - top, 0)) {
            let y = top + pitchAxis.top(p)
            let row = NSRect(x: 0, y: y, width: bounds.width, height: pitchAxis.rowHeight)
            if PitchName.isBlack(p) {
                NSColor.textBackgroundColor.setFill()
                row.fill()
                NSColor.labelColor.withAlphaComponent(p == pressed ? 0.5 : 0.85).setFill()
                NSRect(x: 0, y: y + 1, width: bounds.width * 0.62, height: pitchAxis.rowHeight - 2).fill()
            } else {
                (p == pressed ? NSColor.controlAccentColor.withAlphaComponent(0.4) : NSColor.textBackgroundColor)
                    .setFill()
                row.fill()
            }
            NSColor.separatorColor.setFill()
            NSRect(x: 0, y: y + pitchAxis.rowHeight - 0.5, width: bounds.width, height: 0.5).fill()
            if let label = KeyboardLabels.label(p), pitchAxis.rowHeight >= 8 {
                NSString(string: label).draw(
                    at: CGPoint(x: bounds.width - 26, y: y + (pitchAxis.rowHeight - 11) / 2),
                    withAttributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor])
            }
        }
        NSGraphicsContext.restoreGraphicsState()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Keyboard" }
}
