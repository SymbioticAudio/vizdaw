import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The audio regions of the shown lanes, one line per output path, with
/// mute and solo per path (spec §4b, Plan 4b Ruling 13).
@MainActor
public final class RegionStripView: NSView, TimeAxisFollower {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    public var timeAxis = TimeAxis() { didSet { needsDisplay = true } }
    private var machine = ToolMachine<RegionHit>()
    private var moving: (item: RegionItem, startTick: Int64)?
    private var a11yElements: [CanvasElement] = []

    var side: CGFloat { PianoRollContainer.sideWidth }
    var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }

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
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    func lines() -> [RegionLine] {
        RegionStripLayout.lines(model.regionLanes(), names: model.outputStrips().mapValues(\.name))
    }

    /// The height the container gives it (0 hides it).
    public func wantedHeight() -> CGFloat {
        editor.layout.pianoRoll.showRegions ? RegionStripLayout.height(lines: lines().count) : 0
    }

    // MARK: Drawing

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
        let strips = model.outputStrips()
        let small = NSFont.systemFont(ofSize: 10)
        for (i, line) in lines().enumerated() {
            if let o = line.output {
                let s = strips[o]
                button("M", RegionStripLayout.muteRect(line: i), on: s?.mute ?? false, color: .systemOrange)
                button("S", RegionStripLayout.soloRect(line: i), on: s?.solo ?? false, color: .systemYellow)
            }
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height))
                .addClip()
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(dirty) else { continue }
                let path = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
                NSColor.systemTeal.withAlphaComponent(item.row.mute ? 0.15 : 0.35).setFill()
                path.fill()
                (editor.isSelected(event: item.row.id) ? NSColor.controlAccentColor : NSColor.systemTeal).setStroke()
                path.lineWidth = editor.isSelected(event: item.row.id) ? 2 : 1
                path.stroke()
                NSString(string: model.row(item.lane)?.name ?? "").draw(
                    at: CGPoint(x: r.minX + 4, y: r.minY + 2),
                    withAttributes: [.font: small, .foregroundColor: NSColor.labelColor])
            }
            NSString(string: line.title).draw(
                at: CGPoint(x: side + 4, y: RegionStripLayout.rowHeight * CGFloat(i) + 4),
                withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private func button(_ title: String, _ r: CGRect, on: Bool, color: NSColor) {
        (on ? color : NSColor.controlBackgroundColor).setFill()
        NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3).fill()
        NSColor.separatorColor.setStroke()
        NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 3, yRadius: 3).stroke()
        NSString(string: title).draw(
            at: CGPoint(x: r.minX + 5, y: r.minY + 1),
            withAttributes: [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
    }

    // MARK: Events

    private func point(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    public override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        pressed(at: point(e), clicks: e.clickCount, modifiers: Modifiers(event: e))
    }

    public override func mouseDragged(with e: NSEvent) { dragged(to: point(e), modifiers: Modifiers(event: e)) }
    public override func mouseUp(with e: NSEvent) { released(at: point(e), modifiers: Modifiers(event: e)) }

    // The presses in view coordinates (the events above, and tests).

    func pressed(at p: CGPoint, clicks: Int, modifiers: Modifiers) {
        handle(
            machine.down(
                RegionStripLayout.hit(p, lines: lines(), time: timeAxis, side: side), at: p, clicks: clicks,
                modifiers: modifiers))
    }

    func dragged(to p: CGPoint, modifiers: Modifiers) { handle(machine.drag(to: p, modifiers: modifiers)) }
    func released(at p: CGPoint, modifiers: Modifiers) { handle(machine.up(at: p, modifiers: modifiers)) }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            handle(machine.cancel())
            moving = nil
            if editor.gesture != nil { editor.cancelGesture() }
        } else {
            super.keyDown(with: e)
        }
    }

    /// Leaving the window (⌘W mid-drag) ends the press: its release will
    /// not come here, so the move is cancelled.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        _ = machine.cancel()
        if moving != nil {
            moving = nil
            editor.cancelGesture()
        }
    }

    private func handle(_ effects: [ToolMachine<RegionHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, adding: modifiers.contains(.shift))
            case let .begin(.region(item), from, _):
                guard editor.beginGesture("Move Region", lanes: [item.lane]) else { continue }
                moving = (item, timeAxis.tick(atX: from.x - side))
            case .begin:
                break
            case let .move(_, _, to, _):
                if let m = moving { editor.updateGesture(target(m, to)) }
            case let .end(_, _, to, _):
                if let m = moving {
                    moving = nil
                    editor.endGesture(target(m, to))
                }
            case .cancel:
                if moving != nil {
                    moving = nil
                    editor.cancelGesture()
                }
            }
        }
    }

    private func target(_ m: (item: RegionItem, startTick: Int64), _ p: CGPoint) -> EditCommand? {
        RegionStripLayout.moveCommand(
            m.item, rawTicks: timeAxis.tick(atX: p.x - side) - m.startTick, snap: editor.layout.snap, meter: meter)
    }

    private func click(_ hit: RegionHit, adding: Bool) {
        let strips = model.outputStrips()
        switch hit {
        case .mute(let o): model.setOutputMute(o, !(strips[o]?.mute ?? false))
        case .solo(let o): model.setOutputSolo(o, !(strips[o]?.solo ?? false))
        case .region(let item): model.select([(item.lane, item.row.id)], adding: adding)
        case .none: break
        }
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Audio Regions" }

    public override func accessibilityChildren() -> [Any]? {
        let strips = model.outputStrips()
        var items: [(item: CanvasAccessibilityItem, frame: CGRect)] = []
        for (i, line) in lines().enumerated() {
            if let o = line.output {
                let muted = strips[o]?.mute ?? false
                let soloed = strips[o]?.solo ?? false
                items.append(
                    (CanvasAccessibilityItem(
                        label: "Mute \(line.title)", frame: RegionStripLayout.muteRect(line: i), selected: muted,
                        press: { [weak self] in self?.model.setOutputMute(o, !muted) }),
                    RegionStripLayout.muteRect(line: i)))
                items.append(
                    (CanvasAccessibilityItem(
                        label: "Solo \(line.title)", frame: RegionStripLayout.soloRect(line: i), selected: soloed,
                        press: { [weak self] in self?.model.setOutputSolo(o, !soloed) }),
                    RegionStripLayout.soloRect(line: i)))
            }
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(bounds) else { continue }
                let label = "Region on \(model.row(item.lane)?.name ?? "a lane"), \(line.title)"
                items.append(
                    (CanvasAccessibilityItem(
                        label: label, frame: r, selected: editor.isSelected(event: item.row.id),
                        press: { [weak self] in self?.model.select([(item.lane, item.row.id)], adding: false) }), r))
            }
        }
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}
