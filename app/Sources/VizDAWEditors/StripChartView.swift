import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The target of a row menu's items: one object for the whole menu, each
/// item's tag indexing `lanes` (big menus need no per-item objects).
@MainActor
final class StripRowMenuTarget: NSObject {
    var lanes: [StripLane] = []
    var choose: ((StripLane) -> Void)?

    @objc func pick(_ sender: NSMenuItem) {
        guard lanes.indices.contains(sender.tag) else { return }
        choose?(lanes[sender.tag])
    }
}

/// The Strip Chart (editors spec §4d, Plan 4d Rulings 21–23): the shown
/// lane's values in stacked rows under one ruler; Draw, Modify (lines and
/// Compress rectangles) and Erase strokes; a row's title opens the menu of
/// what it shows.
@MainActor
public final class StripChartView: EditorCanvas {
    public let model: StripChartModel
    var editor: EditorModel { model.editor }
    /// Nil once the document closed (the chart holds it weakly).
    var document: DocumentModel? { model.document }

    static let titleHeight: CGFloat = 18
    static let titleWidth: CGFloat = 200

    /// The press in progress (content coordinates): its row and start.
    private var press: (row: Int, at: CGPoint)?
    private var moved = false
    /// The stroke's pointer path, drawn as its preview.
    private(set) var path: [CGPoint] = []
    /// The target the view last scrolled to.
    private var shownTarget: EditorTarget??
    private let menuTarget = StripRowMenuTarget()

    public init(model: StripChartModel) {
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
            guard let self, let lane = self.model.lane else { return }
            self.editor.selectRange(r, lanes: [lane])
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.model.settings.ticksPerPoint else { return }
            self.model.updateSettings { $0.ticksPerPoint = a.ticksPerPoint }
        }
        menuTarget.choose = nil
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.meter }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange }
    public override var contentHeight: CGFloat { CGFloat(model.rows.count) * rowHeight }
    public override var accessibilityName: String { "Strip Chart" }

    var rowHeight: CGFloat { CGFloat(model.settings.rowHeight) }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document?.playKey
            _ = model.chosenLane
            _ = model.stroke
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
        verticalOffset = clampVertical(verticalOffset)
        reload()
    }

    /// The first layout for a target scrolls to a block's start.
    public override func layout() {
        super.layout()
        let target = model.target
        guard shownTarget != .some(target) else { return }
        shownTarget = .some(target)
        if case .block(_, _, let start, _) = target {
            axis = TimeAxis(ticksPerPoint: axis.ticksPerPoint, originTick: Double(start))
        }
    }

    // MARK: Geometry

    func rowTop(_ i: Int) -> CGFloat { CGFloat(i) * rowHeight - verticalOffset }

    func row(atY y: CGFloat) -> Int? {
        guard y.isFinite, rowHeight > 0 else { return nil }
        let i = Int(((y + verticalOffset) / rowHeight).rounded(.down))
        return model.rows.indices.contains(i) ? i : nil
    }

    func scale(_ i: Int) -> StripScale {
        let lane = model.rows.indices.contains(i) ? model.rows[i] : .velocity
        return StripScale(range: lane.range, top: rowTop(i), height: rowHeight)
    }

    /// A content point as row `i`'s tick and value: snapped to the grid when
    /// snap is on; in Modify mode the operation's amount (Plan 4d Ruling 18).
    func point(_ p: CGPoint, row i: Int) -> StripPoint {
        let raw = axis.tick(atX: p.x)
        let tick = max(snap.enabled ? snap.apply(raw, meter: meter) : raw, 0)
        let s = scale(i)
        let set = model.settings
        let value = set.mode == .modify && set.op != .compress ? s.amount(atY: p.y, op: set.op) : s.value(atY: p.y)
        return StripPoint(tick: tick, value: value)
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let names = model.names
        let selected = Set(editor.selection.events)
        for (i, lane) in model.rows.enumerated() {
            let top = rowTop(i)
            let rect = CGRect(x: 0, y: top, width: content.bounds.width, height: rowHeight)
            guard rect.intersects(dirty) else { continue }
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: rect.maxY - 1, width: dirty.width, height: 1).fill()
            let s = scale(i)
            if let c = lane.center {
                NSColor.separatorColor.withAlphaComponent(0.6).setFill()
                CGRect(x: dirty.minX, y: s.y(c), width: dirty.width, height: 0.5).fill()
            }
            draw(model.values(lane), lane: lane, scale: s, dirty: dirty, selected: selected)
            NSString(string: lane.title(names) + " ▾").draw(
                at: CGPoint(x: 6, y: top + 3),
                withAttributes: [
                    .font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor,
                ])
        }
    }

    private func draw(_ values: [StripValue], lane: StripLane, scale s: StripScale, dirty: CGRect, selected: Set<UInt64>) {
        let accent = NSColor.controlAccentColor
        func isSelected(_ v: StripValue) -> Bool { v.id.map(selected.contains) ?? false }
        if lane.style == .bars {
            let bottom = s.y(s.range.lowerBound)
            for v in values {
                let x0 = axis.x(v.tick)
                guard x0 + 6 >= dirty.minX, x0 <= dirty.maxX else { continue }
                let y = s.y(v.value)
                (isSelected(v) ? accent : accent.withAlphaComponent(0.45)).setFill()
                CGRect(x: x0, y: y, width: 5, height: max(bottom - y, 1)).fill()
            }
            return
        }
        let line = NSBezierPath()
        var prev: StripValue?
        for v in values {
            let p = CGPoint(x: axis.x(v.tick), y: s.y(v.value))
            if let q = prev {
                if !q.ramp { line.line(to: CGPoint(x: p.x, y: s.y(q.value))) }
                line.line(to: p)
            } else {
                line.move(to: CGPoint(x: min(p.x, dirty.minX), y: p.y))
                line.line(to: p)
            }
            prev = v
            (isSelected(v) ? accent : accent.withAlphaComponent(0.8)).setFill()
            CGRect(x: p.x - 2, y: p.y - 2, width: 4, height: 4).fill()
        }
        if let q = prev {
            line.line(to: CGPoint(x: max(dirty.maxX, axis.x(q.tick)), y: s.y(q.value)))
        }
        accent.withAlphaComponent(0.8).setStroke()
        line.lineWidth = 1.5
        line.stroke()
    }

    public override func drawOverlay(in dirty: CGRect) {
        guard let stroke = model.stroke, let first = path.first, let last = path.last else { return }
        let orange = NSColor.systemOrange
        if stroke.compress {
            let r = CGRect(
                x: min(first.x, last.x), y: min(first.y, last.y), width: abs(last.x - first.x),
                height: abs(last.y - first.y))
            orange.withAlphaComponent(0.12).setFill()
            r.fill()
            orange.setStroke()
            NSBezierPath(rect: r).stroke()
            return
        }
        let p = NSBezierPath()
        p.move(to: first)
        if model.settings.shape == .free {
            for q in path.dropFirst() { p.line(to: q) }
        } else {
            p.line(to: last)
        }
        orange.setStroke()
        p.lineWidth = 1
        p.stroke()
    }

    // MARK: Strokes (content coordinates)

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        window?.makeFirstResponder(self)
        guard let i = row(atY: p.y) else { return }
        if p.y - rowTop(i) < Self.titleHeight, p.x < Self.titleWidth {
            rowMenu(i).popUp(positioning: nil, at: p, in: content)
            return
        }
        press = (i, p)
        moved = false
        path = [p]
        model.strokeBegan(row: i, at: point(p, row: i), selectedOnly: modifiers.contains(.option))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        guard let pr = press else { return }
        if !moved {
            guard hypot(p.x - pr.at.x, p.y - pr.at.y) > ToolMachine<Int>.deadZone else { return }
            moved = true
        }
        path.append(p)
        model.strokeMoved(to: point(p, row: pr.row))
        overlay.needsDisplay = true
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        guard let pr = press else { return }
        press = nil
        model.strokeEnded(at: point(p, row: pr.row))
        path = []
        overlay.needsDisplay = true
    }

    /// Escape: the stroke goes without an undo step.
    public override func contentCancel() {
        press = nil
        path = []
        model.strokeCancelled()
        overlay.needsDisplay = true
    }

    /// Leaving the window ends the press: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        contentCancel()
    }

    // MARK: Row menu

    /// What row `i` can show, and Add Row / Remove Row.
    func rowMenu(_ i: Int) -> NSMenu {
        let menu = NSMenu()
        let current = model.rows.indices.contains(i) ? model.rows[i] : .velocity
        let names = model.names
        var lanes: [StripLane] = []
        menuTarget.choose = { [weak self] lane in self?.model.setRow(i, to: lane) }
        func item(_ lane: StripLane, _ title: String? = nil) -> NSMenuItem {
            let it = NSMenuItem(title: title ?? lane.title(names), action: #selector(StripRowMenuTarget.pick(_:)), keyEquivalent: "")
            it.target = menuTarget
            it.tag = lanes.count
            it.state = lane == current ? .on : .off
            lanes.append(lane)
            return it
        }
        func submenu(_ title: String, _ items: [NSMenuItem]) -> NSMenuItem {
            let sub = NSMenu(title: title)
            items.forEach { sub.addItem($0) }
            let it = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            it.submenu = sub
            return it
        }
        for lane in [StripLane.velocity, .releaseVelocity, .duration] { menu.addItem(item(lane)) }
        menu.addItem(.separator())
        var controllers = StripLane.controllers.map { item(.controller($0)) }
        controllers.append(.separator())
        for group in 0..<4 {
            let lo = UInt8(group * 32)
            controllers.append(
                submenu("\(lo)–\(lo + 31)", (lo...lo + 31).map { item(.controller($0), "Controller \($0)") }))
        }
        menu.addItem(submenu("Controller", controllers))
        menu.addItem(item(.pitchBend))
        menu.addItem(item(.channelPressure))
        let octaves = (0..<11).map { octave -> NSMenuItem in
            let lo = UInt8(octave * 12)
            let hi = UInt8(min(octave * 12 + 11, 127))
            return submenu(PitchName.name(lo) + "–" + PitchName.name(hi), (lo...hi).map { item(.polyPressure($0)) })
        }
        menu.addItem(submenu("Poly Pressure", octaves))
        menu.addItem(item(.program))
        menu.addItem(item(.tempo))
        menu.addItem(.separator())
        let groups = model.menus.map { g in submenu(g.title, g.lanes.map { item($0) }) }
        let automation = submenu("Automation", groups)
        automation.isEnabled = !groups.isEmpty
        menu.addItem(automation)
        menu.addItem(.separator())
        let add = MenuAction(title: "Add Row") { [weak self] in self?.model.addRow() }.item(checked: false)
        add.isEnabled = model.rows.count < StripChartModel.maxRows
        menu.addItem(add)
        let remove = MenuAction(title: "Remove Row") { [weak self] in self?.model.removeRow(i) }.item(checked: false)
        remove.isEnabled = model.rows.count > 1
        menu.addItem(remove)
        menuTarget.lanes = lanes
        menu.autoenablesItems = false
        return menu
    }

    // MARK: Accessibility

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        var out: [CanvasAccessibilityItem] = []
        let names = model.names
        for (i, lane) in model.rows.enumerated() {
            let s = scale(i)
            for v in model.values(lane).prefix(200) {
                let b = Bbu.of(v.tick, meter: meter)
                let frame = CGRect(x: axis.x(v.tick) - 3, y: s.y(v.value) - 3, width: 6, height: 6)
                out.append(
                    CanvasAccessibilityItem(
                        label: "\(lane.title(names)) \(lane.text(v.value, names)), bar \(b.bar) beat \(b.beat)",
                        frame: frame, selected: v.id.map { editor.isSelected(event: $0) } ?? false, press: nil))
            }
        }
        return out
    }
}
