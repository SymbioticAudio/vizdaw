import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// What a press in the drum grid landed on: a row, by layout indexes.
public struct DrumPress: Equatable, Sendable {
    public let lane: Int
    public let row: Int
}

/// Accessibility labels of drum cells and hits (Plan 4e Ruling 12).
public enum DrumA11y {
    /// "Kick, bar 2 beat 3 tick 1920, on, velocity 100".
    public static func label(row: String, tick: Int64, hit: Bool, velocity: UInt16, meter: [MeterEntryRow]) -> String {
        let bar = MeterMath.bar(at: tick, meter)
        let beatTicks = MeterMath.beatTicks(bar.meter)
        let into = tick - bar.start
        var s = "\(row), bar \(bar.number) beat \(into / beatTicks + 1)"
        if into % beatTicks != 0 { s += " tick \(into % beatTicks)" }
        s += hit ? ", on, velocity \(velocity >> 9)" : ", off"
        return s
    }

    /// The most cells and hits the grid lists to VoiceOver and the control
    /// channel (the visible ones).
    public static let maxItems = 2_048
}

/// The drum grid's hits (spec §4e): drum lanes as bands of rows over a
/// shared time axis, a step matrix of cells or a timeline of hits, the
/// paint tool and velocity lanes. What a point is over, the cells, the
/// strokes and their commands come from Rust through `DrumGridModel`;
/// this view draws and forwards the pointer.
@MainActor
public final class DrumGridView: EditorCanvas {
    public let model: DrumGridModel
    var editor: EditorModel { model.editor }
    var document: DocumentModel? { model.document }

    private var machine = ToolMachine<DrumPress?>()
    /// The target the view last scrolled to.
    private var scrolledTo: EditorTarget??

    public init(model: DrumGridModel) {
        self.model = model
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: model.settings.ticksPerPoint)
        playheadSource = { [weak self] now in
            guard let self, let doc = self.document, let seq = self.model.seq, doc.transport.seq == seq else {
                return nil
            }
            return doc.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [weak self] r in
            guard let self, let seq = self.model.seq else { return }
            self.editor.selectRange(r, lanes: self.model.lanes.map { LaneKey(seq, $0.lane) })
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.editor.layout.drumGrid.ticksPerPoint else { return }
            self.editor.updateLayout { $0.drumGrid.ticksPerPoint = a.ticksPerPoint }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange ?? model.focusRange }
    public override var contentHeight: CGFloat { CGFloat(model.layout.height) }
    public override var accessibilityName: String { "Drum Grid" }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document?.playKey
            _ = model.selectedRow
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        let s = model.settings
        if s.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: s.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        let key = document?.playKey
        setPlaying(key?.playing == true && key?.seq == model.seq)
        if let t = scrolledTo, t != model.target { needsLayout = true }
        verticalOffset = clampVertical(verticalOffset)
        reload()
    }

    /// The first layout, and the first after the target changed, scrolls
    /// to a block target.
    public override func layout() {
        super.layout()
        let target = model.target
        guard scrolledTo != .some(target), content.bounds.width > 0 else { return }
        scrolledTo = .some(target)
        if let r = model.focusRange {
            axis = axis.scrolled(toTick: Double(r.start) - 16 * axis.ticksPerPoint)
        }
    }

    // MARK: Geometry

    /// A band's top in content coordinates.
    func top(_ b: DrumBandRow) -> CGFloat { CGFloat(b.top) - verticalOffset }

    /// A hit is at least this wide on screen, in ticks.
    var grab: Int64 { Int64((4 * axis.ticksPerPoint).rounded(.up)) }

    /// The rect of matrix cell `tick` in a band.
    func cellRect(_ tick: Int64, _ b: DrumBandRow) -> CGRect {
        let x0 = axis.x(tick)
        let x1 = axis.x(tick + max(model.settings.step, 1))
        return CGRect(x: x0, y: top(b), width: max(x1 - x0, 1), height: CGFloat(b.cells))
    }

    /// The rect of a hit in the timeline.
    func hitRect(_ h: DrumHitRow, _ b: DrumBandRow) -> CGRect {
        let x0 = axis.x(h.tick)
        return CGRect(x: x0, y: top(b), width: max(axis.width(ofTicks: h.duration), 4), height: CGFloat(b.cells))
    }

    private func visibleBands(_ dirty: CGRect) -> [DrumBandRow] {
        model.layout.bands.filter { b in
            let t = top(b)
            return t + CGFloat(b.height) >= dirty.minY && t <= dirty.maxY
        }
    }

    private func cells(_ r: DrumRowRow, _ dirty: CGRect) -> [DrumCellRow] {
        (try? drumGridCells(
            hits: r.hits, from: axis.tick(atX: dirty.minX), to: axis.tick(atX: dirty.maxX) + 1,
            step: model.settings.step)) ?? []
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let matrix = model.settings.view == .matrix
        for b in visibleBands(dirty) {
            let y = top(b)
            switch b.kind {
            case .header:
                NSColor.windowBackgroundColor.setFill()
                CGRect(x: dirty.minX, y: y, width: dirty.width, height: CGFloat(b.height)).fill()
            case let .row(lane, row):
                guard let r = model.row(Int(lane), Int(row)) else { continue }
                if model.selectedRow == DrumRowKey(lane: model.lanes[Int(lane)].lane, row: r.id) {
                    NSColor.controlAccentColor.withAlphaComponent(0.08).setFill()
                    CGRect(x: dirty.minX, y: y, width: dirty.width, height: CGFloat(b.height)).fill()
                }
                if matrix {
                    drawCells(r, b, dirty)
                } else {
                    drawHits(r, b, dirty)
                }
                if b.height > b.cells { drawVelocities(r, b, dirty) }
            case .newLane:
                NSString(string: "Drop a row here to give it a lane of its own").draw(
                    at: CGPoint(x: max(dirty.minX, 0) + 8, y: y + 7),
                    withAttributes: [
                        .font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.tertiaryLabelColor,
                    ])
            }
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: y + CGFloat(b.height) - 0.5, width: dirty.width, height: 1).fill()
        }
    }

    private func drawCells(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        for c in cells(r, dirty) {
            let rect = cellRect(c.tick, b).insetBy(dx: 1, dy: 2)
            let path = NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2)
            if c.hits.isEmpty {
                NSColor.quaternaryLabelColor.withAlphaComponent(0.15).setFill()
                path.fill()
            } else {
                let strength = 0.35 + 0.65 * CGFloat(c.velocity) / 65_535
                NSColor.systemOrange.withAlphaComponent(strength).setFill()
                path.fill()
                if c.hits.contains(where: editor.isSelected(event:)) {
                    NSColor.controlAccentColor.setStroke()
                    path.lineWidth = 2
                    path.stroke()
                }
            }
        }
    }

    private func drawHits(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        for h in r.hits {
            let rect = hitRect(h, b).insetBy(dx: 0, dy: 2)
            guard rect.maxX >= dirty.minX, rect.minX <= dirty.maxX else { continue }
            let path = NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2)
            NSColor.systemOrange.withAlphaComponent(h.mute ? 0.3 : 0.85).setFill()
            path.fill()
            if editor.isSelected(event: h.id) {
                NSColor.controlAccentColor.setStroke()
                path.lineWidth = 2
                path.stroke()
            }
        }
    }

    private func drawVelocities(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        let laneTop = top(b) + CGFloat(b.cells)
        let h = CGFloat(b.height - b.cells) - 4
        NSColor.labelColor.withAlphaComponent(0.04).setFill()
        CGRect(x: dirty.minX, y: laneTop, width: dirty.width, height: h + 4).fill()
        NSColor.systemOrange.setFill()
        for hit in r.hits {
            let x = axis.x(hit.tick)
            guard x >= dirty.minX - 4, x <= dirty.maxX else { continue }
            let bar = max(h * CGFloat(hit.velocity) / 65_535, 1)
            CGRect(x: x, y: laneTop + 2 + h - bar, width: 3, height: bar).fill()
        }
    }

    // MARK: Gestures

    /// The tick under `x`, and on the shared edit grid.
    private func ticks(_ x: CGFloat) -> (at: Int64, snapped: Int64) {
        let t = axis.tick(atX: x)
        return (t, snap.floor(t, meter: meter))
    }

    private func velocity(_ press: DrumPress, y: CGFloat) -> UInt8? {
        (try? drumRowVelocity(
            layout: model.layout, lane: UInt32(press.lane), row: UInt32(press.row), y: Double(y + verticalOffset)))
            ?? nil
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        model.becameActive()
        var press: DrumPress?
        if case let .row(lane, row, _) = model.spot(Double(p.y + verticalOffset)) {
            press = DrumPress(lane: Int(lane), row: Int(row))
            model.select(lane: Int(lane), row: Int(row))
        }
        handle(machine.down(press, at: p, clicks: clicks, modifiers: modifiers))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
    }

    /// Escape; also aborts a gesture whose final edit still waits on Busy.
    public override func contentCancel() {
        handle(machine.cancel())
        model.strokeCancelled()
        if editor.gesture != nil { editor.cancelGesture() }
    }

    private func begin(_ press: DrumPress, at p: CGPoint) {
        let t = ticks(p.x)
        model.strokeBegan(
            lane: press.lane, row: press.row, at: t.at, snapped: t.snapped, grab: grab,
            velocity: velocity(press, y: p.y), erase: editor.tool == .eraser)
    }

    private func handle(_ effects: [ToolMachine<DrumPress?>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(press?, at, _, _):
                begin(press, at: at)
                model.strokeEnded(at: ticks(at.x).at, grab: grab, velocity: velocity(press, y: at.y))
            case let .begin(press?, from, _):
                begin(press, at: from)
            case let .move(press?, _, to, _):
                model.strokeMoved(at: ticks(to.x).at, grab: grab, velocity: velocity(press, y: to.y))
            case let .end(press?, _, to, _):
                model.strokeEnded(at: ticks(to.x).at, grab: grab, velocity: velocity(press, y: to.y))
            case .cancel:
                model.strokeCancelled()
            case .click(nil, _, _, _), .begin(nil, _, _), .move(nil, _, _, _), .end(nil, _, _, _):
                break
            }
        }
    }

    /// Leaving the window ends the press: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        _ = machine.cancel()
        model.strokeCancelled()
    }

    // MARK: Accessibility (Plan 4e Ruling 12)

    /// The visible cells (matrix) or hits (timeline) of every visible row;
    /// pressing one toggles it like a click.
    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        let visible = content.bounds
        let matrix = model.settings.view == .matrix
        var out: [CanvasAccessibilityItem] = []
        for b in visibleBands(visible) {
            guard case let .row(lane, row) = b.kind, let r = model.row(Int(lane), Int(row)) else { continue }
            let press = DrumPress(lane: Int(lane), row: Int(row))
            let items: [(tick: Int64, hit: Bool, velocity: UInt16, frame: CGRect)] =
                matrix
                ? cells(r, visible).map { c in (c.tick, !c.hits.isEmpty, c.velocity, cellRect(c.tick, b)) }
                : r.hits.map { h in (h.tick, true, h.velocity, hitRect(h, b)) }.filter { $0.frame.intersects(visible) }
            for i in items {
                guard out.count < DrumA11y.maxItems else { return out }
                let frame = i.frame.intersection(visible)
                guard !frame.isEmpty else { continue }
                let tick = i.tick
                out.append(
                    CanvasAccessibilityItem(
                        label: DrumA11y.label(row: r.name, tick: tick, hit: i.hit, velocity: i.velocity, meter: meter),
                        frame: frame, selected: false,
                        press: { [weak self] in self?.toggle(press, tick: tick) }))
            }
        }
        return out
    }

    /// A click at `tick` on a row (VoiceOver's press).
    func toggle(_ press: DrumPress, tick: Int64) {
        model.becameActive()
        model.strokeBegan(
            lane: press.lane, row: press.row, at: tick, snapped: tick, grab: 1, velocity: nil,
            erase: editor.tool == .eraser)
        model.strokeEnded(at: tick, grab: 1, velocity: nil)
    }
}
