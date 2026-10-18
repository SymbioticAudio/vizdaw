import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// Lane colours (Plan 4b Ruling 9).
@MainActor
public enum PianoRollPalette {
    static let colors: [NSColor] = [
        .systemBlue, .systemOrange, .systemGreen, .systemPink, .systemPurple, .systemTeal, .systemYellow,
        .systemBrown,
    ]

    public static func color(_ index: Int) -> NSColor {
        colors[((index % colors.count) + colors.count) % colors.count]
    }
}

/// The piano roll's note area (spec §4b, parent §9): notes of the shown
/// lanes over a pitch axis, cursor zones, pencil, eraser, rubber band,
/// notes that sound while pressed and dragged.
@MainActor
public final class PianoRollView: EditorCanvas {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    /// Nil once the document closed (the roll holds it weakly).
    var document: DocumentModel? { model.document }

    public private(set) var pitchAxis = PitchAxis()
    /// The keyboard follows the vertical scroll and row height.
    public var onPitchScroll: ((PitchAxis) -> Void)?
    /// The region strip and expression pane follow the time axis.
    public var onTimeAxis: ((TimeAxis) -> Void)?

    private var machine = ToolMachine<PianoRollHit>()
    /// The target the view last centred on: another one (a different
    /// block opened in this roll) centres and scrolls again.
    private var centeredOn: EditorTarget??
    private var tracking: NSTrackingArea?

    private enum Drag {
        case notes(NoteDrag, startTick: Int64, startY: CGFloat)
        case pencil(lane: LaneKey, start: Int64, pitch: UInt8)
        /// The notes erased so far and the last pointer position.
        case erase([UInt64], last: CGPoint)
        case band(from: CGPoint, adding: Bool)
        /// A ⌘-drag of a note's end (Plan 4d Ruling 23).
        case scale(ScaleTimeDrag, startTick: Int64)
    }

    private var drag: Drag?
    /// The rubber band being drawn, in content coordinates.
    private(set) var band: CGRect?

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        let settings = model.editor.layout.pianoRoll
        pitchAxis = PitchAxis(rowHeight: CGFloat(settings.rowHeight))
        axis = TimeAxis(ticksPerPoint: settings.ticksPerPoint)
        playheadSource = { [weak self] now in
            guard let self, let doc = self.document, let seq = self.model.seq, doc.transport.seq == seq else {
                return nil
            }
            return doc.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [weak self] r in
            guard let self else { return }
            self.editor.selectRange(r, lanes: self.model.shownLanes)
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onAxisChange = { [weak self] a in
            guard let self else { return }
            self.onTimeAxis?(a)
            if a.ticksPerPoint != self.editor.layout.pianoRoll.ticksPerPoint {
                self.editor.updateLayout { $0.pianoRoll.ticksPerPoint = a.ticksPerPoint }
            }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange ?? model.focusRange }
    public override var contentHeight: CGFloat { pitchAxis.contentHeight }
    public override var accessibilityName: String { "Piano Roll" }

    public override var verticalOffset: CGFloat {
        didSet {
            pitchAxis.offset = verticalOffset
            onPitchScroll?(pitchAxis)
        }
    }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = editor.tool
            _ = document?.playKey
            _ = model.chosenLane
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        let s = editor.layout.pianoRoll
        if CGFloat(s.rowHeight) != pitchAxis.rowHeight {
            pitchAxis = pitchAxis.withRowHeight(CGFloat(s.rowHeight), keepingY: content.bounds.height / 2)
            verticalOffset = clampVertical(pitchAxis.offset)
            onPitchScroll?(pitchAxis)
        }
        if s.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: s.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        let key = document?.playKey
        setPlaying(key?.playing == true && key?.seq == model.seq)
        if let c = centeredOn, c != model.target { needsLayout = true }
        reload()
    }

    /// The first layout, and the first after the target changed, centres
    /// the shown notes and scrolls to a block target (Plan 4b Ruling 10).
    public override func layout() {
        super.layout()
        let target = model.target
        guard centeredOn != .some(target), content.bounds.height > 0 else { return }
        centeredOn = .some(target)
        let pitches = model.shownLanes.flatMap { model.notes($0) }.map(\.pitch)
        var middle: UInt8 = 60
        if let lo = pitches.min(), let hi = pitches.max() { middle = UInt8((Int(lo) + Int(hi)) / 2) }
        verticalOffset = clampVertical(pitchAxis.centering(middle, height: content.bounds.height).offset)
        if let r = model.focusRange {
            axis = axis.scrolled(toTick: Double(r.start) - 16 * axis.ticksPerPoint)
        }
        onTimeAxis?(axis)
    }

    public func laneNotes() -> [LaneNotes] {
        model.shownLanes.map { LaneNotes(lane: $0, notes: model.notes($0)) }
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        drawRows(dirty)
        let active = model.editLane
        for l in paintOrder() { draw(l, active: l.lane == active, dirty) }
    }

    /// The shown lanes bottom to top: the others in reverse order, then the
    /// active lane. `PianoRollGeometry.hit` tries the active lane, then the
    /// others in order, so the note hit is always the one drawn on top.
    func paintOrder() -> [LaneNotes] {
        let lanes = laneNotes()
        let active = model.editLane
        return lanes.filter { $0.lane != active }.reversed() + lanes.filter { $0.lane == active }
    }

    /// Black-key rows shaded, a line under every C.
    private func drawRows(_ dirty: CGRect) {
        for p in pitchAxis.visible(height: content.bounds.height) {
            let y = pitchAxis.top(p)
            if PitchName.isBlack(p) {
                NSColor.labelColor.withAlphaComponent(0.05).setFill()
                CGRect(x: dirty.minX, y: y, width: dirty.width, height: pitchAxis.rowHeight).fill()
            }
            if p % 12 == 0 {
                NSColor.separatorColor.setFill()
                CGRect(x: dirty.minX, y: y + pitchAxis.rowHeight - 0.5, width: dirty.width, height: 1).fill()
            }
        }
    }

    private func draw(_ l: LaneNotes, active: Bool, _ dirty: CGRect) {
        let color = PianoRollPalette.color(model.colorIndex(l.lane))
        let from = axis.tick(atX: dirty.minX - PianoRollGeometry.minNoteWidth)
        let to = axis.tick(atX: dirty.maxX)
        for n in l.notes where n.tick <= to && n.tick + n.duration >= from {
            let r = PianoRollGeometry.rect(n, time: axis, pitch: pitchAxis)
            guard r.intersects(dirty) else { continue }
            let path = NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 2, yRadius: 2)
            color.withAlphaComponent((active ? 0.85 : 0.4) * (n.mute ? 0.4 : 1)).setFill()
            path.fill()
            let selected = editor.isSelected(event: n.id)
            (selected ? NSColor.controlAccentColor : (color.shadow(withLevel: 0.4) ?? color)).setStroke()
            path.lineWidth = selected ? 2 : 1
            path.stroke()
        }
    }

    public override func drawOverlay(in dirty: CGRect) {
        guard let band else { return }
        NSColor.controlAccentColor.withAlphaComponent(0.12).setFill()
        band.fill()
        NSColor.controlAccentColor.setStroke()
        NSBezierPath(rect: band.insetBy(dx: 0.5, dy: 0.5)).stroke()
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> PianoRollHit {
        PianoRollGeometry.hit(p, lanes: laneNotes(), active: model.editLane, time: axis, pitch: pitchAxis)
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        let h = hit(p)
        handle(machine.down(h, at: p, clicks: clicks, modifiers: modifiers))
        pressDown(h)
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
        model.silence()
    }

    /// Escape; also aborts a gesture whose final edit still waits on Busy.
    public override func contentCancel() {
        handle(machine.cancel())
        abandonDrag()
        model.silence()
        if editor.gesture != nil { editor.cancelGesture() }
    }

    /// A pressed note sounds until release; the pencil sounds its new note
    /// (Plan 4b Ruling 6).
    private func pressDown(_ h: PianoRollHit) {
        switch (editor.tool, h) {
        case (.eraser, _):
            break
        case (_, .note(let n)):
            model.chosenLane = n.lane
            model.sound(n.row.pitch, velocity: n.row.velocity, dest: model.dest(of: n.row, in: n.lane))
        case (.pencil, .empty(_, let pitch)):
            if let lane = model.editLane {
                model.sound(pitch, velocity: Pencil.velocity, dest: model.row(lane)?.midiDest)
            }
        case (_, .empty):
            break
        }
    }

    private func handle(_ effects: [ToolMachine<PianoRollHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, modifiers: modifiers)
            case let .begin(hit, from, modifiers):
                begin(hit, from: from, modifiers: modifiers)
            case let .move(_, _, to, _):
                move(to: to)
            case let .end(_, _, to, _):
                end(at: to)
            case .cancel:
                abandonDrag()
            }
        }
    }

    private func click(_ hit: PianoRollHit, modifiers: Modifiers) {
        switch (editor.tool, hit) {
        case (.eraser, .note(let n)):
            editor.edit(.event(edit: .delete(events: [n.row.id])))
        case (_, .note(let n)):
            if modifiers.contains(.shift) {
                model.toggle(n.row.id, in: n.lane)
            } else {
                model.select([(n.lane, n.row.id)], adding: false)
            }
        case (.pencil, .empty(let tick, let pitch)):
            // A gesture, so the undo step reads "Insert Note" (Plan Ruling 7).
            guard let lane = model.editLane, openGesture("Insert Note", lanes: [lane], origin: .other) else { return }
            let start = Pencil.start(tick, snap: snap, meter: meter)
            editor.endGesture(
                Pencil.command(
                    lane: lane, tick: start, pitch: pitch, length: Pencil.step(at: start, snap: snap, meter: meter)))
        case (_, .empty):
            if !modifiers.contains(.shift) { editor.clearSelection() }
        }
    }

    private func begin(_ hit: PianoRollHit, from: CGPoint, modifiers: Modifiers) {
        switch (editor.tool, hit) {
        case (.eraser, _):
            let erased = Eraser.adding(hit, to: [])
            guard openGesture("Erase Notes", lanes: Set(model.shownLanes), origin: .other) else { return }
            drag = .erase(erased, last: from)
            editor.updateGesture(Eraser.command(erased))
        case (_, .note(let n)):
            if !editor.isSelected(event: n.row.id) { model.select([(n.lane, n.row.id)], adding: false) }
            let lanes = laneNotes()
            let ids = NoteDrag.events(pressed: n.row.id, selected: Set(editor.selection.events), lanes: lanes)
            let idSet = Set(ids)
            let pitches = lanes.flatMap(\.notes).filter { idSet.contains($0.id) }.map(\.pitch)
            let d = NoteDrag(
                zone: n.zone, pressed: n.row, lane: n.lane, events: ids, lowest: pitches.min() ?? n.row.pitch,
                highest: pitches.max() ?? n.row.pitch, copy: modifiers.contains(.option))
            let touched = Set(lanes.filter { l in l.notes.contains { idSet.contains($0.id) } }.map(\.lane))
            // ⌘ on the end zone scales the dragged notes' time.
            if modifiers.contains(.command), n.zone == .end,
                let sd = ScaleTimeDrag(notes: lanes.flatMap(\.notes).filter { idSet.contains($0.id) })
            {
                guard openGesture(ScaleTimeDrag.label, lanes: touched, origin: .notes) else { return }
                drag = .scale(sd, startTick: axis.tick(atX: from.x))
                return
            }
            guard openGesture(d.label, lanes: touched, origin: .notes) else { return }
            drag = .notes(d, startTick: axis.tick(atX: from.x), startY: from.y)
        case (.pencil, .empty(let tick, let pitch)):
            guard let lane = model.editLane, openGesture("Insert Note", lanes: [lane], origin: .other) else { return }
            drag = .pencil(lane: lane, start: Pencil.start(tick, snap: snap, meter: meter), pitch: pitch)
        case (_, .empty):
            drag = .band(from: from, adding: modifiers.contains(.shift))
        }
    }

    /// Opens the gesture a pencil click or drag needs. Refused while
    /// another one is still finishing: the status says so and the note
    /// sounded on mouse-down stops.
    private func openGesture(_ label: String, lanes: Set<LaneKey>, origin: GestureOrigin) -> Bool {
        let pending = editor.gesture != nil
        if editor.beginGesture(label, lanes: lanes, origin: origin) { return true }
        if pending { editor.status = EditorModel.gestureOpenMessage }
        model.silence()
        return false
    }

    /// The notes the eraser crossed between two drag points.
    private func crossed(from a: CGPoint, to b: CGPoint) -> [UInt64] {
        PianoRollGeometry.notes(crossingFrom: a, to: b, lanes: laneNotes(), time: axis, pitch: pitchAxis)
    }

    private func target(_ d: Drag, at p: CGPoint) -> EditCommand? {
        switch d {
        case let .notes(nd, startTick, startY):
            let raw = axis.tick(atX: p.x) - startTick
            // Without horizontal motion a resize changes nothing (a zero-length
            // note would otherwise gain a tick).
            if nd.zone == .end, raw == 0 { return nil }
            return nd.command(
                rawTicks: raw, dy: p.y - startY, rowHeight: pitchAxis.rowHeight, snap: snap, meter: meter)
        case let .pencil(lane, start, pitch):
            let length = Pencil.length(start: start, to: axis.tick(atX: p.x), snap: snap, meter: meter)
            return Pencil.command(lane: lane, tick: start, pitch: pitch, length: length)
        case let .erase(erased, last):
            return Eraser.command(Eraser.adding(crossed(from: last, to: p), to: erased))
        case let .scale(sd, startTick):
            return sd.command(rawTicks: axis.tick(atX: p.x) - startTick, snap: snap, meter: meter)
        case .band:
            return nil
        }
    }

    private func move(to p: CGPoint) {
        guard let d = drag else { return }
        switch d {
        case let .notes(nd, _, startY):
            editor.updateGesture(target(d, at: p))
            model.sound(
                nd.soundingPitch(dy: p.y - startY, rowHeight: pitchAxis.rowHeight), velocity: nd.pressed.velocity,
                dest: model.dest(of: nd.pressed, in: nd.lane))
        case .pencil, .scale:
            editor.updateGesture(target(d, at: p))
        case let .erase(erased, last):
            let next = Eraser.adding(crossed(from: last, to: p), to: erased)
            drag = .erase(next, last: p)
            guard next != erased else { return }
            editor.updateGesture(Eraser.command(next))
        case let .band(from, _):
            band = RubberBand.rect(from: from, to: p)
            overlay.needsDisplay = true
        }
    }

    private func end(at p: CGPoint) {
        guard let d = drag else { return }
        drag = nil
        if case let .band(from, adding) = d {
            clearBand()
            let rect = RubberBand.rect(from: from, to: p)
            if editor.exactMode {
                let start = axis.tick(atX: rect.minX)
                let end = max(axis.tick(atX: rect.maxX), start + 1)
                let a = pitchAxis.pitch(atY: rect.minY)
                let b = pitchAxis.pitch(atY: rect.maxY)
                editor.requestExact(
                    ExactMode.selectRequest(
                        range: TickRange(start: start, end: end), pitches: min(a, b)...max(a, b),
                        lanes: model.shownLanes, adding: adding))
                return
            }
            let found = PianoRollGeometry.notes(
                in: rect, lanes: laneNotes(), time: axis, pitch: pitchAxis)
            model.select(found, adding: adding)
            return
        }
        editor.endGesture(target(d, at: p))
    }

    /// Leaving the window ends the press: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        _ = machine.cancel()
        abandonDrag()
        model.silence()
    }

    /// Drops the drag in progress; its gesture (a rubber band has none)
    /// goes without an undo step.
    private func abandonDrag() {
        guard let d = drag else { return }
        drag = nil
        if case .band = d {
            clearBand()
        } else {
            editor.cancelGesture()
        }
    }

    private func clearBand() {
        guard band != nil else { return }
        band = nil
        overlay.needsDisplay = true
    }

    // MARK: Keys and cursors

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 51 || e.keyCode == 117 {  // Delete, Forward Delete
            model.deleteSelected()
        } else {
            super.keyDown(with: e)
        }
    }

    public override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = tracking { removeTrackingArea(t) }
        let t = NSTrackingArea(
            rect: .zero, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(t)
        tracking = t
    }

    public override func mouseMoved(with e: NSEvent) {
        let p = content.convert(e.locationInWindow, from: nil)
        guard p.y >= 0 else {
            NSCursor.arrow.set()
            return
        }
        Self.cursor(PianoRollCursor.kind(for: hit(p), tool: editor.tool)).set()
    }

    public static func cursor(_ k: CursorKind) -> NSCursor {
        switch k {
        case .arrow: .arrow
        case .openHand: .openHand
        case .upDown: .resizeUpDown
        case .resizeRight: .resizeRight
        case .crosshair: .crosshair
        case .eraser: .disappearingItem
        }
    }

    // MARK: Accessibility (Plan 4b Ruling 21)

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        let several = model.shownLanes.count > 1
        var out: [CanvasAccessibilityItem] = []
        for l in laneNotes() {
            let name = several ? model.row(l.lane)?.name : nil
            for n in l.notes {
                let r = PianoRollGeometry.rect(n, time: axis, pitch: pitchAxis)
                guard r.intersects(content.bounds) else { continue }
                let key = l.lane
                let id = n.id
                out.append(
                    CanvasAccessibilityItem(
                        label: PianoRollA11y.label(n, laneName: name, meter: meter), frame: r,
                        selected: editor.isSelected(event: id),
                        press: { [weak self] in self?.model.select([(key, id)], adding: false) }))
            }
        }
        return out
    }
}
