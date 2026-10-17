import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The arrangement timeline (spec §4a, parent §9): the shown sequence's
/// lanes as rows, their blocks, reference blocks labelled with their
/// sequence, block drags (move, ⌥ copy, merge into another lane),
/// double-click to open, light drawing for big projects.
@MainActor
public final class ArrangementView: EditorCanvas {
    public let document: DocumentModel
    var editor: EditorModel { document.editor }

    private var machine = ToolMachine<ArrangementHit>()
    private(set) var geometry = ArrangementGeometry()
    /// The last drawing was in light mode.
    public private(set) var light = false

    private struct Drag {
        let hit: BlockHit
        let events: [UInt64]
        let rows: [EventRow]
        let copy: Bool
        let startTick: Int64
    }

    private var drag: Drag?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: document.editor.layout.ticksPerPoint)
        playheadSource = { [unowned self] now in
            let t = self.document.transport
            guard let seq = self.seq, t.seq == seq else { return nil }
            return self.document.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [unowned self] r in self.editor.selectRange(r, lanes: self.lanes) }
        onRulerClick = { [unowned self] t in self.document.locate(t) }
        onToggleSnap = { [unowned self] in self.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [unowned self] a in
            if a.ticksPerPoint != self.editor.layout.ticksPerPoint {
                self.editor.updateLayout { $0.ticksPerPoint = a.ticksPerPoint }
            }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    var seq: UInt64? { document.selectedSeq }

    var lanes: [LaneKey] {
        guard let s = seq else { return [] }
        return document.lanes.map { LaneKey(s, $0.id) }
    }

    public override var meter: [MeterEntryRow] { seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange }
    public override var contentHeight: CGFloat { geometry.contentHeight(rows: lanes.count) }
    public override var accessibilityName: String { "Arrangement" }

    /// Redraws whenever drawn document or editor state changes.
    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document.lanes
            _ = document.selectedSeq
            // Not `transport`: it changes on every poll; the playhead moves
            // without redrawing.
            _ = document.playKey
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        geometry.laneHeight = CGFloat(editor.layout.laneHeight)
        if editor.layout.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: editor.layout.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        setPlaying(document.playKey.playing && document.playKey.seq == seq)
        reload()
    }

    public override var verticalOffset: CGFloat {
        didSet { geometry.verticalOffset = verticalOffset }
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let keys = lanes
        var rows: [(Int, LaneKey, [BlockRow])] = []
        var visible = 0
        for (i, key) in keys.enumerated() {
            let top = geometry.rowTop(i)
            guard top + geometry.laneHeight >= dirty.minY, top <= dirty.maxY else { continue }
            let shown = editor.blocks(key).filter {
                geometry.blockRect($0, row: i, axis: axis).intersects(dirty)
            }
            visible += shown.count
            rows.append((i, key, shown))
        }
        light = LightDrawing.isLight(editor.layout.lightMode, visibleBlocks: visible)
        for (i, key, blocks) in rows {
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: geometry.rowTop(i + 1) - 0.5, width: dirty.width, height: 1).fill()
            for b in blocks { draw(b, row: i, key: key) }
        }
    }

    private func draw(_ b: BlockRow, row: Int, key: LaneKey) {
        let r = geometry.blockRect(b, row: row, axis: axis)
        let selected = editor.isSelected(b)
        let color: NSColor = b.kind == .reference ? .systemPurple : .systemBlue
        let shape = NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 3, yRadius: 3)
        if !light {
            color.withAlphaComponent(b.muted ? 0.12 : 0.30).setFill()
            shape.fill()
        }
        (selected ? NSColor.controlAccentColor : color).setStroke()
        shape.lineWidth = selected ? 2 : 1
        if b.muted { shape.setLineDash([3, 2], count: 2, phase: 0) }
        shape.stroke()
        guard !light, r.width > 24 else { return }
        if b.kind == .reference {
            let text = NSString(string: "↻ " + (b.label ?? ""))
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: r.insetBy(dx: 3, dy: 1)).addClip()
            text.draw(
                at: CGPoint(x: r.minX + 4, y: r.minY + 3),
                withAttributes: [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.labelColor])
            NSGraphicsContext.restoreGraphicsState()
        } else {
            sketch(b, in: r, key: key)
        }
    }

    /// Mini notes inside an events block (normal drawing only).
    private func sketch(_ b: BlockRow, in r: CGRect, key: LaneKey) {
        guard let rows = editor.laneEvents(key)?.events else { return }
        var lo = 0
        var hi = rows.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if rows[mid].tick < b.start { lo = mid + 1 } else { hi = mid }
        }
        let notes = rows[lo...].prefix(while: { $0.tick <= b.end }).filter { $0.kind == .note }
        guard let minP = notes.map(\.pitch).min(), let maxP = notes.map(\.pitch).max() else { return }
        let span = CGFloat(max(Int(maxP) - Int(minP), 1))
        NSColor.labelColor.withAlphaComponent(0.55).setFill()
        for n in notes {
            let y = r.maxY - 5 - CGFloat(Int(n.pitch) - Int(minP)) / span * (r.height - 10)
            let x = axis.x(n.tick)
            CGRect(x: x, y: y - 1, width: max(axis.width(ofTicks: n.duration), 1), height: 2)
                .intersection(r).fill()
        }
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> ArrangementHit {
        let keys = lanes
        var map: [LaneKey: [BlockRow]] = [:]
        if let row = geometry.row(atY: p.y, count: keys.count) { map[keys[row]] = editor.blocks(keys[row]) }
        return ArrangementHitTest.hit(p, lanes: keys, blocks: map, geometry: geometry, axis: axis)
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        handle(machine.down(hit(p), at: p, clicks: clicks, modifiers: modifiers))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
    }

    /// Escape. Also aborts a gesture whose final edit is still pending on
    /// Busy: the machine is idle again after mouse-up, but the edit is open.
    public override func contentCancel() {
        handle(machine.cancel())
        drag = nil
        if editor.gesture != nil { editor.cancelGesture() }
    }

    private func handle(_ effects: [ToolMachine<ArrangementHit>.Effect]) {
        for (i, fx) in effects.enumerated() {
            switch fx {
            case let .click(hit, _, clicks, modifiers):
                click(hit, clicks: clicks, modifiers: modifiers)
            case let .begin(.block(b), from, modifiers):
                // The first move arrives with the begin: its row is the
                // target lane the gesture starts out touching.
                var to = from
                if i + 1 < effects.count, case let .move(_, _, p, _) = effects[i + 1] { to = p }
                beginDrag(b, from: from, to: to, copy: modifiers.contains(.option))
            case let .move(_, _, to, _):
                if let d = drag {
                    editor.setGestureLanes(touched(d, to: to))
                    editor.updateGesture(target(d, to: to))
                }
            case let .end(_, _, to, _):
                if let d = drag {
                    drag = nil
                    editor.setGestureLanes(touched(d, to: to))
                    editor.endGesture(target(d, to: to))
                }
            case .cancel:
                if drag != nil {
                    drag = nil
                    editor.cancelGesture()
                }
            case .begin(.empty, _, _):
                break
            }
        }
    }

    private func click(_ hit: ArrangementHit, clicks: Int, modifiers: Modifiers) {
        switch hit {
        case .block(let b) where editor.tool == .eraser:
            editor.edit(.event(edit: .delete(events: b.block.events)))
        case .block(let b) where clicks >= 2:
            if b.block.kind == .reference {
                editor.openReference(b.block)
            } else {
                editor.openDetail(.block(seq: b.lane.seq, lane: b.lane.lane, start: b.block.start, end: b.block.end))
            }
        case .block(let b):
            editor.selectBlock(b.block, in: b.lane, adding: modifiers.contains(.shift))
        case .empty:
            if !modifiers.contains(.shift) { editor.clearSelection() }
        }
    }

    private func beginDrag(_ b: BlockHit, from: CGPoint, to: CGPoint, copy: Bool) {
        guard editor.tool == .arrow else { return }
        let selected = Set(editor.selection.events)
        let events = BlockDrag.events(pressed: b.block, laneBlocks: editor.blocks(b.lane), selected: selected)
        let rows = copy ? (editor.laneEvents(b.lane)?.events ?? []) : []
        let d = Drag(hit: b, events: events, rows: rows, copy: copy, startTick: axis.tick(atX: from.x))
        guard editor.beginGesture(BlockDrag.label(copy: copy), lanes: touched(d, to: to)) else { return }
        drag = d
    }

    /// The lane under `to` (the source lane when outside every row).
    private func targetLane(_ d: Drag, to: CGPoint) -> LaneKey {
        let keys = lanes
        return geometry.row(atY: to.y, count: keys.count).map { keys[$0] } ?? d.hit.lane
    }

    /// The lanes the drag edits with the pointer at `to`: source and target.
    private func touched(_ d: Drag, to: CGPoint) -> Set<LaneKey> {
        [d.hit.lane, targetLane(d, to: to)]
    }

    private func target(_ d: Drag, to: CGPoint) -> EditCommand? {
        let raw = axis.tick(atX: to.x) - d.startTick
        let delta = BlockDrag.delta(rawTicks: raw, blockStart: d.hit.block.start, snap: snap, meter: meter)
        return BlockDrag.command(
            events: d.events, rows: d.rows, from: d.hit.lane, to: targetLane(d, to: to), delta: delta,
            copy: d.copy)
    }

    // MARK: Accessibility

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        var out: [CanvasAccessibilityItem] = []
        let rows = document.lanes
        for (i, key) in lanes.enumerated() where i < rows.count {
            for b in editor.blocks(key) {
                let r = geometry.blockRect(b, row: i, axis: axis)
                guard r.intersects(content.bounds) else { continue }
                out.append(
                    CanvasAccessibilityItem(
                        label: ArrangementA11y.label(b, laneName: rows[i].name, meter: meter), frame: r,
                        selected: editor.isSelected(b),
                        press: { [unowned self] in self.editor.selectBlock(b, in: key, adding: false) }))
            }
        }
        return out
    }
}
