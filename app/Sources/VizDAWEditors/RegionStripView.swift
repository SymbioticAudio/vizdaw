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
    /// A fade or gain handle being dragged (plan 5b Ruling 7).
    private var handleDrag: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint)?
    /// The gain readout while the gain band is dragged.
    private var readout: (text: String, at: CGPoint)?
    private var a11yElements: [CanvasElement] = []

    var side: CGFloat { PianoRollContainer.sideWidth }
    var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        registerForDraggedTypes([.fileURL])
        observe()
    }

    // MARK: Dropping audio files (spec §10.2; plan 5b Ruling 8)

    public override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        model.seq != nil && !AudioDrop.paths(sender.draggingPasteboard).isEmpty ? .copy : []
    }

    public override func draggingUpdated(_ sender: any NSDraggingInfo) -> NSDragOperation {
        draggingEntered(sender)
    }

    public override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        drop(AudioDrop.paths(sender.draggingPasteboard), at: convert(sender.draggingLocation, from: nil))
    }

    /// Imports `paths` dropped at `p`: into the lane of the region under
    /// it, else the piano roll's edit lane (new lanes without one).
    @discardableResult
    func drop(_ paths: [String], at p: CGPoint) -> Bool {
        guard !paths.isEmpty, let seq = model.seq else { return false }
        let lane = RegionStripLayout.dropLane(p, lines: lines(), time: timeAxis, side: side) ?? model.editLane
        let tick = AudioDrop.tick(timeAxis.tick(atX: p.x - side), snap: editor.layout.snap, meter: meter)
        model.document?.importAudio(
            paths: paths, target: ImportTargetRow(seq: seq, lane: lane?.seq == seq ? lane?.lane : nil, tick: tick))
        return true
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
            // Waveforms arrive, media go offline or come back, handles
            // follow the tool (preflight M8).
            _ = editor.tool
            _ = model.document?.overviewVersion
            _ = model.document?.mediaVersion
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
            let media = model.document?.media
            let lineArea = NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height)
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(dirty) else { continue }
                let vis = r.intersection(dirty).intersection(lineArea)
                let offline = media?.offline(item.row.media) != nil
                var rows: OverviewRows?
                if !offline, !vis.isNull, vis.width > 0 {
                    let from = timeAxis.tick(atX: vis.minX - side)
                    rows = media?.waveform(
                        event: item.row.id, fromTick: from, toTick: max(timeAxis.tick(atX: vis.maxX - side), from + 1),
                        columns: Int(vis.width.rounded(.up)))
                }
                let wave = vis.isNull
                    ? CGRect.zero
                    : CGRect(
                        x: vis.minX, y: r.minY + RegionHandles.band, width: vis.width,
                        height: max(r.height - RegionHandles.band - 1, 1))
                RegionDrawing.draw(
                    item.row, in: r, rows: rows, waveRect: wave, offline: offline,
                    selected: editor.isSelected(event: item.row.id), handles: editor.tool == .arrow)
                NSString(string: model.row(item.lane)?.name ?? "").draw(
                    at: CGPoint(x: r.minX + 4, y: r.minY + 2),
                    withAttributes: [.font: small, .foregroundColor: NSColor.labelColor])
            }
            NSString(string: line.title).draw(
                at: CGPoint(x: side + 4, y: RegionStripLayout.rowHeight * CGFloat(i) + 4),
                withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
            NSGraphicsContext.restoreGraphicsState()
        }
        if let r = readout { RegionDrawing.readout(r.text, at: r.at) }
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
        handle(machine.down(hitForPress(at: p), at: p, clicks: clicks, modifiers: modifiers))
    }

    /// What a press at `p` grabs: a handle first (arrow tool only, as in
    /// the arrangement; preflight M8), then the strip's own hit.
    func hitForPress(at p: CGPoint) -> RegionHit {
        let ls = lines()
        let handle =
            editor.tool == .arrow ? RegionStripLayout.handleHit(p, lines: ls, time: timeAxis, side: side) : nil
        return handle ?? RegionStripLayout.hit(p, lines: ls, time: timeAxis, side: side)
    }

    func dragged(to p: CGPoint, modifiers: Modifiers) { handle(machine.drag(to: p, modifiers: modifiers)) }
    func released(at p: CGPoint, modifiers: Modifiers) { handle(machine.up(at: p, modifiers: modifiers)) }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            handle(machine.cancel())
            moving = nil
            handleDrag = nil
            clearReadout()
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
        if moving != nil || handleDrag != nil {
            moving = nil
            handleDrag = nil
            clearReadout()
            editor.cancelGesture()
        }
    }

    private func handle(_ effects: [ToolMachine<RegionHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, modifiers: modifiers)
            case let .begin(.region(item), from, _):
                guard editor.beginGesture("Move Region", lanes: [item.lane], origin: .regionStrip) else { continue }
                moving = (item, timeAxis.tick(atX: from.x - side))
            case let .begin(.handle(item, h, rect), from, _):
                guard editor.beginGesture(RegionHandles.label(h), lanes: [item.lane], origin: .other) else { continue }
                handleDrag = (item, h, rect, from)
            case .begin:
                break
            case let .move(_, _, to, modifiers):
                if let m = moving {
                    editor.updateGesture(target(m, to))
                } else if let d = handleDrag {
                    showReadout(d, to: to, modifiers: modifiers)
                    editor.updateGesture(handleTarget(d, to: to, modifiers: modifiers))
                }
            case let .end(_, _, to, modifiers):
                if let m = moving {
                    moving = nil
                    editor.endGesture(target(m, to))
                } else if let d = handleDrag {
                    handleDrag = nil
                    clearReadout()
                    editor.endGesture(handleTarget(d, to: to, modifiers: modifiers))
                }
            case .cancel:
                if moving != nil || handleDrag != nil {
                    moving = nil
                    handleDrag = nil
                    clearReadout()
                    editor.cancelGesture()
                }
            }
        }
    }

    private func handleTarget(
        _ d: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint), to: CGPoint,
        modifiers: Modifiers
    ) -> EditCommand? {
        RegionHandles.target(
            d.item.row, d.handle, rect: d.rect, dx: to.x - d.from.x, dy: to.y - d.from.y,
            fine: modifiers.contains(.shift))
    }

    private func showReadout(
        _ d: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint), to: CGPoint,
        modifiers: Modifiers
    ) {
        guard d.handle == .gain else { return }
        let g = RegionHandles.gain(d.item.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        readout = (RegionHandles.gainLabel(g), CGPoint(x: to.x + 12, y: max(to.y - 18, 0)))
        needsDisplay = true
    }

    private func clearReadout() {
        guard readout != nil else { return }
        readout = nil
        needsDisplay = true
    }

    /// The fade-shape menu of a region's fade area.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = point(event)
        guard p.x >= side, p.y >= 0 else { return nil }
        let ls = lines()
        let i = Int((p.y / RegionStripLayout.rowHeight).rounded(.down))
        guard i < ls.count else { return nil }
        for item in ls[i].items.reversed() {
            let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
            if let h = RegionHandles.fadeArea(p, row: item.row, in: r) {
                return RegionDrawing.fadeShapeMenu(item.row, h) { [weak self] c in self?.editor.edit(c) }
            }
        }
        return nil
    }

    private func target(_ m: (item: RegionItem, startTick: Int64), _ p: CGPoint) -> EditCommand? {
        RegionStripLayout.moveCommand(
            m.item, rawTicks: timeAxis.tick(atX: p.x - side) - m.startTick, snap: editor.layout.snap, meter: meter)
    }

    private func click(_ hit: RegionHit, modifiers: Modifiers) {
        let strips = model.outputStrips()
        switch hit {
        case .mute(let o): model.setOutputMute(o, !(strips[o]?.mute ?? false))
        case .solo(let o): model.setOutputSolo(o, !(strips[o]?.solo ?? false))
        case .region(let item): model.select([(item.lane, item.row.id)], adding: modifiers.contains(.shift))
        case .handle(let item, .gain, _) where modifiers.contains(.option):
            if let c = RegionHandles.resetGain(item.row) { editor.edit(c) }
        case .handle(let item, _, _): model.select([(item.lane, item.row.id)], adding: modifiers.contains(.shift))
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
        let media = model.document?.media
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
                // Preflight M8: gain and offline, as in the arrangement.
                let label = RegionA11y.label(
                    item.row, name: media?.row(item.row.media)?.name ?? "audio",
                    laneName: model.row(item.lane)?.name ?? "a lane", meter: meter,
                    offline: media?.offline(item.row.media) != nil)
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
