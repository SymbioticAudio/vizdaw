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
    /// A fade or gain handle being dragged (plan 5b Ruling 7).
    private var handleDrag: (hit: RegionHandleHit, from: CGPoint)?
    /// The gain readout while the gain band is dragged.
    private var readout: (text: String, at: CGPoint)?

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
            // Handles are drawn for the arrow tool only (review I1).
            _ = editor.tool
            // Waveforms arrive and media go offline or come back.
            _ = document.overviewVersion
            _ = document.mediaVersion
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

    /// How many observed changes have refreshed the view (a test seam:
    /// AppKit keeps no reliable needsDisplay flag off screen).
    private(set) var refreshes = 0

    func refresh() {
        refreshes += 1
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

    /// The blocks of the rows in `rect`, per row, that intersect it.
    private func blocks(in rect: CGRect) -> [(Int, LaneKey, [BlockRow])] {
        var rows: [(Int, LaneKey, [BlockRow])] = []
        for (i, key) in lanes.enumerated() {
            let top = geometry.rowTop(i)
            guard top + geometry.laneHeight >= rect.minY, top <= rect.maxY else { continue }
            let shown = editor.blocks(key).filter {
                geometry.blockRect($0, row: i, axis: axis).intersects(rect)
            }
            rows.append((i, key, shown))
        }
        return rows
    }

    /// How many blocks the whole visible area shows (Plan Ruling 17): light
    /// mode depends on this, never on the dirty rect, so a partial redraw
    /// cannot mix the two modes.
    var visibleBlockCount: Int {
        blocks(in: content.bounds).reduce(0) { $0 + $1.2.count }
    }

    public override func drawContent(in dirty: CGRect) {
        light = LightDrawing.isLight(editor.layout.lightMode, visibleBlocks: visibleBlockCount)
        for (i, key, blocks) in blocks(in: dirty) {
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: geometry.rowTop(i + 1) - 0.5, width: dirty.width, height: 1).fill()
            for b in blocks { draw(b, row: i, key: key, dirty: dirty) }
        }
    }

    private func draw(_ b: BlockRow, row: Int, key: LaneKey, dirty: CGRect) {
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
        if !light, b.kind == .events { drawRegions(b, row: row, key: key, dirty: dirty) }
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

    /// The audio regions of an events block: waveform, fades, gain and
    /// handles (spec §10.1–10.2).
    private func drawRegions(_ b: BlockRow, row: Int, key: LaneKey, dirty: CGRect) {
        let ids = Set(b.events)
        let media = document.media
        for r in regionRows(key) where ids.contains(r.id) {
            let rect = geometry.regionRect(r, row: row, axis: axis)
            let vis = rect.intersection(dirty).intersection(content.bounds)
            guard !vis.isNull, vis.width > 0 else { continue }
            let offline = media.offline(r.media) != nil
            var rows: OverviewRows?
            if !offline {
                let from = axis.tick(atX: vis.minX)
                rows = media.waveform(
                    event: r.id, fromTick: from, toTick: max(axis.tick(atX: vis.maxX), from + 1),
                    columns: Int(vis.width.rounded(.up)))
            }
            let wave = CGRect(
                x: vis.minX, y: rect.minY + RegionHandles.band, width: vis.width,
                height: max(rect.height - RegionHandles.band - 2, 1))
            RegionDrawing.draw(
                r, in: rect, rows: rows, waveRect: wave, offline: offline, selected: editor.isSelected(event: r.id),
                handles: editor.tool == .arrow)
        }
    }

    /// The audio regions of a lane's active take.
    private func regionRows(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .audioRegion } ?? []
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> ArrangementHit {
        if let h = handleHit(p) { return .handle(h) }
        return blockHit(p)
    }

    private func blockHit(_ p: CGPoint) -> ArrangementHit {
        let keys = lanes
        var map: [LaneKey: [BlockRow]] = [:]
        if let row = geometry.row(atY: p.y, count: keys.count) { map[keys[row]] = editor.blocks(keys[row]) }
        return ArrangementHitTest.hit(p, lanes: keys, blocks: map, geometry: geometry, axis: axis)
    }

    /// A region's fade or gain handle under `p` (arrow tool, normal
    /// drawing; plan 5b Ruling 7).
    private func handleHit(_ p: CGPoint) -> RegionHandleHit? {
        guard editor.tool == .arrow, !light else { return nil }
        let keys = lanes
        guard let row = geometry.row(atY: p.y, count: keys.count) else { return nil }
        for r in regionRows(keys[row]).reversed() {
            let rect = geometry.regionRect(r, row: row, axis: axis)
            if let h = RegionHandles.hit(p, row: r, in: rect) {
                return RegionHandleHit(lane: keys[row], row: r, handle: h, rect: rect)
            }
        }
        return nil
    }

    /// The fade whose shape a context menu at `p` sets.
    private func fadeArea(_ p: CGPoint) -> (row: EventRow, handle: RegionHandle)? {
        guard !light else { return nil }
        let keys = lanes
        guard let row = geometry.row(atY: p.y, count: keys.count) else { return nil }
        for r in regionRows(keys[row]).reversed() {
            if let h = RegionHandles.fadeArea(p, row: r, in: geometry.regionRect(r, row: row, axis: axis)) {
                return (r, h)
            }
        }
        return nil
    }

    private func handleTarget(_ d: (hit: RegionHandleHit, from: CGPoint), to: CGPoint, modifiers: Modifiers)
        -> EditCommand?
    {
        RegionHandles.target(
            d.hit.row, d.hit.handle, rect: d.hit.rect, dx: to.x - d.from.x, dy: to.y - d.from.y,
            fine: modifiers.contains(.shift))
    }

    private func showReadout(_ d: (hit: RegionHandleHit, from: CGPoint), to: CGPoint, modifiers: Modifiers) {
        guard d.hit.handle == .gain else { return }
        let g = RegionHandles.gain(d.hit.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        readout = (RegionHandles.gainLabel(g), CGPoint(x: to.x + 12, y: to.y - 20))
        overlay.needsDisplay = true
    }

    private func clearReadout() {
        guard readout != nil else { return }
        readout = nil
        overlay.needsDisplay = true
    }

    public override func drawOverlay(in dirty: CGRect) {
        if let r = readout { RegionDrawing.readout(r.text, at: r.at) }
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        // A double-click opens the block even on a region's handles
        // (review m1).
        handle(machine.down(clicks >= 2 ? blockHit(p) : hit(p), at: p, clicks: clicks, modifiers: modifiers))
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
        handleDrag = nil
        clearReadout()
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
            case let .begin(.handle(h), from, _):
                guard editor.beginGesture(RegionHandles.label(h.handle), lanes: [h.lane], origin: .other) else {
                    continue
                }
                handleDrag = (h, from)
            case let .move(_, _, to, modifiers):
                if let d = drag {
                    editor.setGestureLanes(touched(d, to: to))
                    editor.updateGesture(target(d, to: to))
                } else if let h = handleDrag {
                    showReadout(h, to: to, modifiers: modifiers)
                    editor.updateGesture(handleTarget(h, to: to, modifiers: modifiers))
                }
            case let .end(_, _, to, modifiers):
                if let d = drag {
                    drag = nil
                    editor.setGestureLanes(touched(d, to: to))
                    editor.endGesture(target(d, to: to))
                } else if let h = handleDrag {
                    handleDrag = nil
                    clearReadout()
                    editor.endGesture(handleTarget(h, to: to, modifiers: modifiers))
                }
            case .cancel:
                if drag != nil {
                    drag = nil
                    editor.cancelGesture()
                }
                if handleDrag != nil {
                    handleDrag = nil
                    clearReadout()
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
        case .handle(let h) where h.handle == .gain && modifiers.contains(.option):
            if let c = RegionHandles.resetGain(h.row) { editor.edit(c) }
        case .handle(let h):
            // ⇧ adds, as in the region strip (review m2).
            var s = modifiers.contains(.shift) ? editor.selection : SelectionRow(timeRange: nil, lanes: [], events: [])
            if !s.events.contains(h.row.id) { s.events.append(h.row.id) }
            if !s.lanes.contains(h.lane.ref) { s.lanes.append(h.lane.ref) }
            editor.setSelection(s)
        }
    }

    private func beginDrag(_ b: BlockHit, from: CGPoint, to: CGPoint, copy: Bool) {
        guard editor.tool == .arrow else { return }
        let selected = Set(editor.selection.events)
        let events = BlockDrag.events(pressed: b.block, laneBlocks: editor.blocks(b.lane), selected: selected)
        let rows = copy ? (editor.laneEvents(b.lane)?.events ?? []) : []
        let d = Drag(hit: b, events: events, rows: rows, copy: copy, startTick: axis.tick(atX: from.x))
        guard editor.beginGesture(BlockDrag.label(copy: copy), lanes: touched(d, to: to), origin: .blocks) else { return }
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

    // MARK: Context menus (spec §4a subsequence workflow)

    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = content.convert(event.locationInWindow, from: nil)
        if let area = fadeArea(p) {
            return RegionDrawing.fadeShapeMenu(area.row, area.handle) { [weak self] c in self?.editor.edit(c) }
        }
        guard case .block(let b) = blockHit(p) else { return nil }
        if !editor.isSelected(b.block) { editor.selectBlock(b.block, in: b.lane, adding: false) }
        let menu = NSMenu()
        func add(_ title: String, _ run: @escaping @MainActor () -> Void) {
            menu.addItem(MenuAction(title: title, run: run).item(checked: false))
        }
        let block = b.block
        let key = b.lane
        if block.kind == .reference, let event = block.events.first {
            add("Open Sequence") { [weak self] in self?.editor.openReference(block) }
            add("Parameters…") { [weak self] in self?.editor.editReference(block, in: key) }
            add("Make Private Copy") { [weak self] in self?.editor.makePrivateCopy(event) }
            add("Flatten") { [weak self] in self?.editor.flatten(event) }
            let sub = NSMenu()
            for s in EditorModel.rePointTargets(excluding: block.refSeq, among: document.snapshot.sequences) {
                sub.addItem(
                    MenuAction(title: s.name) { [weak self] in self?.editor.rePoint(event, to: s.id) }
                        .item(checked: false))
            }
            let rePoint = NSMenuItem(title: "Re-point To", action: nil, keyEquivalent: "")
            rePoint.submenu = sub
            menu.addItem(rePoint)
        } else {
            for kind in DetailEditorKind.allCases {
                add("Open in \(kind.title)") { [weak self] in
                    self?.editor.openDetail(
                        .block(seq: key.seq, lane: key.lane, start: block.start, end: block.end), kind: kind)
                }
            }
            add("Make Reference") { [weak self] in self?.editor.makeReference(from: block, in: key) }
        }
        menu.addItem(.separator())
        add("Delete") { [weak self] in self?.editor.edit(.event(edit: .delete(events: block.events))) }
        return menu
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
                        press: { [weak self] in self?.editor.selectBlock(b, in: key, adding: false) }))
            }
            let media = document.media
            for r in regionRows(key) {
                let rect = geometry.regionRect(r, row: i, axis: axis)
                guard rect.intersects(content.bounds) else { continue }
                out.append(
                    CanvasAccessibilityItem(
                        label: RegionA11y.label(
                            r, name: media.row(r.media)?.name ?? "audio", laneName: rows[i].name, meter: meter,
                            offline: media.offline(r.media) != nil),
                        frame: rect, selected: editor.isSelected(event: r.id),
                        press: { [weak self] in
                            self?.editor.setSelection(SelectionRow(timeRange: nil, lanes: [key.ref], events: [r.id]))
                        }))
            }
        }
        return out
    }
}
