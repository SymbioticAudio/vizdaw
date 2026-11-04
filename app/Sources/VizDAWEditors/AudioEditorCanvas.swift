import AppKit
import Observation
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The audio editor's canvas (plan 7c Rulings 3, 6, 8 and 12): the ruler
/// on top, the level scale at the left and, in the wave rect, the file's
/// waveform, the region shaped by its gain and fades, the envelope line,
/// clip marks and the handles. Flipped: the ruler is at the smaller y, and
/// so is 0 dB in each channel's band. `press`, `drag`, `release` and
/// `cancelKey` are the mouse and Escape paths, and the tests' seams.
@MainActor
public final class AudioEditorCanvas: NSView, ThemedView {
    public let document: DocumentModel
    var editor: EditorModel { document.editor }
    var model: AudioEditorModel { document.audioEditor }

    /// The theme it draws in (plan 7b Ruling 12): its host sets it; a
    /// change redraws it.
    public var theme: Theme = ThemedViews.current {
        didSet { themeDidChange(from: oldValue) }
    }

    /// How often a redraw was asked for (a test seam: off screen AppKit
    /// reads `needsDisplay` back as true even after it is cleared).
    private(set) var displayRequests = 0

    public override var needsDisplay: Bool {
        didSet { if needsDisplay { displayRequests += 1 } }
    }

    /// A press on a handle: everything the drag's target is computed from,
    /// as it was at the press (Ruling 6).
    private struct Press {
        let handle: AudioEditorHandle
        let region: EditedRegion
        let from: CGPoint
        let axis: FrameAxis
        let wave: CGRect
        let mediaFrames: UInt64
        /// Read once, at the press (Ruling 13.8).
        let startRoom: UInt64
        let option: Bool
        /// Past the dead zone: the gesture is open.
        var began = false
    }

    private var pressed: Press?
    /// The gain readout while the gain band is dragged.
    private var readout: (text: String, at: CGPoint)?
    /// VoiceOver children, kept so their identity is stable between queries.
    private var a11yElements: [CanvasElement] = []

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    private var wave: CGRect { AudioEditorGeometry.waveRect(in: bounds) }

    private static func mediaFrames(_ r: EditedRegion) -> UInt64 {
        r.media?.frames ?? r.row.sourceOffset.addingReportingOverflow(r.row.frames).partialValue
    }

    // MARK: Observation and sync (pre-flight I4)

    /// Redraws, and refits a newly shown region, whenever what it draws
    /// changes: the arrangement's set plus the editor's axis, height and
    /// shown region.
    private func observe() {
        withObservationTracking {
            _ = document.snapshot
            _ = document.lanes
            _ = document.selectedSeq
            _ = editor.selection
            _ = editor.drawVersion
            // Offline hatching.
            _ = document.mediaVersion
            // The lane's strip colour.
            _ = document.mixerVersion
            _ = document.storeMixer?.snapshot
            // Pending and Coarse waveforms redraw when they arrive.
            _ = document.overviewVersion
            _ = model.axis
            _ = model.amplitude
            _ = model.shownRegion
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    /// `sync` assigns only what changed, so this cannot retrigger itself.
    private func refresh() {
        model.sync(width: wave.width)
        needsDisplay = true
    }

    public override func layout() {
        super.layout()
        refresh()
    }

    // MARK: Gestures (Ruling 6)

    public override func mouseDown(with e: NSEvent) {
        press(convert(e.locationInWindow, from: nil), clicks: e.clickCount, modifiers: e.modifierFlags)
    }

    public override func mouseDragged(with e: NSEvent) {
        drag(convert(e.locationInWindow, from: nil), modifiers: e.modifierFlags)
    }

    public override func mouseUp(with e: NSEvent) {
        release(convert(e.locationInWindow, from: nil), modifiers: e.modifierFlags)
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            cancelKey()
        } else {
            super.keyDown(with: e)
        }
    }

    public override func cancelOperation(_ sender: Any?) { cancelKey() }

    /// A press grabs the handle under `p`, if any. The canvas becomes the
    /// first responder, so Escape reaches it.
    public func press(_ p: CGPoint, clicks: Int, modifiers: NSEvent.ModifierFlags) {
        window?.makeFirstResponder(self)
        pressed = nil
        readout = nil
        guard let r = document.editedRegion else { return }
        let wave = self.wave
        let axis = model.axis
        guard let h = AudioEditorGeometry.hit(p, row: r.row, axis: axis, in: wave) else { return }
        pressed = Press(
            handle: h, region: r, from: p, axis: axis, wave: wave, mediaFrames: Self.mediaFrames(r),
            startRoom: h == .start ? model.startRoom : 0, option: modifiers.contains(.option))
        needsDisplay = true
    }

    /// Past the 3-point dead zone the gesture opens; each step sends the
    /// full target from the row at the press.
    public func drag(_ p: CGPoint, modifiers: NSEvent.ModifierFlags) {
        guard var d = pressed else { return }
        if !d.began {
            guard hypot(p.x - d.from.x, p.y - d.from.y) > ToolMachine<Int>.deadZone else { return }
            guard editor.beginGesture(AudioEditorGeometry.label(d.handle), lanes: [d.region.lane], origin: .other)
            else {
                pressed = nil
                needsDisplay = true
                return
            }
            d.began = true
            pressed = d
        }
        editor.updateGesture(target(d, to: p, modifiers: modifiers))
        if d.handle == .gain {
            let g = RegionHandles.gain(d.region.row, dy: p.y - d.from.y, fine: modifiers.contains(.shift))
            let at = CGPoint(x: p.x + theme.spacing.m, y: max(p.y - theme.spacing.xl, d.wave.minY))
            readout = (AudioEditorEntry.gainText(Double(g)), at)
        }
        needsDisplay = true
    }

    /// Ends the gesture; a ⌥-click on the gain band (no drag) resets the gain.
    public func release(_ p: CGPoint, modifiers: NSEvent.ModifierFlags) {
        guard let d = pressed else { return }
        pressed = nil
        readout = nil
        if d.began {
            editor.endGesture(target(d, to: p, modifiers: modifiers))
        } else if d.option, d.handle == .gain, let c = RegionHandles.resetGain(d.region.row) {
            _ = editor.edit(c)
        }
        needsDisplay = true
    }

    /// Escape: the drag is cancelled, and so is a gesture whose final edit
    /// is still pending on Busy.
    public func cancelKey() {
        let began = pressed?.began ?? false
        pressed = nil
        readout = nil
        if began || editor.gesture != nil { editor.cancelGesture() }
        needsDisplay = true
    }

    private func target(_ d: Press, to p: CGPoint, modifiers: NSEvent.ModifierFlags) -> EditCommand? {
        AudioEditorGeometry.target(
            d.region.row, d.handle, dx: p.x - d.from.x, dy: p.y - d.from.y, fine: modifiers.contains(.shift),
            axis: d.axis, mediaFrames: d.mediaFrames, startRoom: d.startRoom, in: d.wave)
    }

    // MARK: Wheel, pinch and the shape menu

    public override func scrollWheel(with e: NSEvent) {
        let x = convert(e.locationInWindow, from: nil).x - wave.minX
        let precise = e.hasPreciseScrollingDeltas
        if e.modifierFlags.contains(.command) {
            model.zoom(by: WheelScroll.zoomFactor(dy: e.scrollingDeltaY, precise: precise), aroundX: x)
            return
        }
        let d = WheelScroll.points(dx: e.scrollingDeltaX, dy: e.scrollingDeltaY, precise: precise)
        // The editor scrolls in time only: a plain wheel's vertical turn
        // scrolls it too.
        model.scroll(byPoints: -(abs(d.dx) >= abs(d.dy) ? d.dx : d.dy))
    }

    public override func magnify(with e: NSEvent) {
        model.zoom(by: 1 + Double(e.magnification), aroundX: convert(e.locationInWindow, from: nil).x - wave.minX)
    }

    /// A right-click in a fade's area: its shapes.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        let wave = self.wave
        guard wave.contains(p), let r = document.editedRegion else { return nil }
        let rect = AudioEditorGeometry.regionRect(r.row, axis: model.axis, in: wave)
        guard let h = RegionHandles.fadeArea(p, row: r.row, in: rect) else { return nil }
        return RegionDrawing.fadeShapeMenu(r.row, h) { [weak self] c in _ = self?.editor.edit(c) }
    }

    // MARK: Drawing (Rulings 3, 8 and 12)

    public override func draw(_ dirtyRect: NSRect) {
        let wave = self.wave
        let ruler = CGRect(
            x: bounds.minX, y: bounds.minY, width: bounds.width,
            height: min(AudioEditorGeometry.rulerHeight, bounds.height))
        let gutter = CGRect(x: bounds.minX, y: wave.minY, width: wave.minX - bounds.minX, height: wave.height)
        theme.canvas.setFill()
        bounds.fill()
        theme.rulerBackground.setFill()
        ruler.fill()
        gutter.fill()
        theme.separatorLine.setFill()
        CGRect(x: bounds.minX, y: ruler.maxY - 1, width: bounds.width, height: 1).fill()
        CGRect(x: gutter.maxX - 1, y: gutter.minY, width: 1, height: gutter.height).fill()
        guard let r = document.editedRegion, wave.width > 0, wave.height > 0 else { return }
        let axis = model.axis
        let amplitude = model.amplitude
        let media = Self.mediaFrames(r)
        let channels = max(Int(r.media?.channels ?? 1), 1)
        drawOutside(axis: axis, media: media, in: wave)
        drawRuler(axis: axis, media: media, rate: r.media?.sampleRate ?? 0, in: ruler, wave: wave)
        drawScale(amplitude: amplitude, channels: channels, gutter: gutter, wave: wave)
        if let span = AudioEditorGeometry.span(axis: axis, width: wave.width, mediaFrames: media) {
            drawAudio(r, span: span, axis: axis, amplitude: amplitude, channels: channels, in: wave)
        }
        drawHandles(r, axis: axis, in: wave)
        if let readout { RegionDrawing.readout(readout.text, at: readout.at, theme: theme) }
    }

    /// 1. Beyond the file, in `audioOutside`.
    private func drawOutside(axis: FrameAxis, media: UInt64, in wave: CGRect) {
        let x0 = wave.minX + axis.x(0)
        let x1 = wave.minX + axis.x(Double(media))
        guard x0.isFinite, x1.isFinite else { return }
        theme.audioOutside.setFill()
        if x0 > wave.minX {
            CGRect(x: wave.minX, y: wave.minY, width: min(x0, wave.maxX) - wave.minX, height: wave.height).fill()
        }
        if x1 < wave.maxX {
            let from = max(x1, wave.minX)
            CGRect(x: from, y: wave.minY, width: wave.maxX - from, height: wave.height).fill()
        }
    }

    private var captionAttributes: [NSAttributedString.Key: Any] {
        [.font: theme.digitsFont(theme.type.caption), .foregroundColor: theme.audioScaleText]
    }

    /// The least gap between two ruler labels (the marks' own spacing).
    private static let rulerLabelGap: CGFloat = 80

    /// 2. The ruler: marks inside the file only, labels never closer than
    /// 80 points.
    private func drawRuler(axis: FrameAxis, media: UInt64, rate: UInt32, in ruler: CGRect, wave: CGRect) {
        let attrs = captionAttributes
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: CGRect(x: wave.minX, y: ruler.minY, width: wave.width, height: ruler.height)).addClip()
        var lastLabel = -CGFloat.infinity
        let tick = theme.spacing.xs
        for m in AudioEditorGeometry.rulerMarks(axis: axis, width: wave.width, sampleRate: rate) {
            guard m.x >= 0, axis.frame(atX: m.x) <= Double(media) + 0.5 else { continue }
            let x = wave.minX + m.x
            theme.audioScaleLine.setFill()
            CGRect(x: x, y: ruler.maxY - tick, width: 1, height: tick).fill()
            guard x - lastLabel >= Self.rulerLabelGap else { continue }
            lastLabel = x
            let s = NSString(string: m.label)
            let h = s.size(withAttributes: attrs).height
            s.draw(
                at: CGPoint(x: x + theme.spacing.xxs, y: ruler.minY + max((ruler.height - tick - h) / 2, 0)),
                withAttributes: attrs)
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    /// 3. The level scale: labels in the gutter, lines across the wave. A
    /// label held inside the wave (the top mark's) can reach the next one:
    /// that next label is left out, its line kept.
    private func drawScale(amplitude: Int, channels: Int, gutter: CGRect, wave: CGRect) {
        let attrs = captionAttributes
        var last: CGRect?
        for mark in AudioEditorGeometry.scaleMarks(amplitude: amplitude, channels: channels, in: wave) {
            theme.audioScaleLine.setFill()
            CGRect(x: wave.minX, y: mark.y - 0.5, width: wave.width, height: 1).fill()
            let s = NSString(string: mark.label)
            let size = s.size(withAttributes: attrs)
            let y = min(max(mark.y - size.height / 2, wave.minY), wave.maxY - size.height)
            let box = CGRect(x: gutter.maxX - theme.spacing.xs - size.width, y: y, width: size.width, height: size.height)
            if let last, box.minY < last.maxY { continue }
            last = box
            s.draw(at: box.origin, withAttributes: attrs)
        }
    }

    /// 4–8. The file's waveform over the span, the region's fill, its
    /// shaped waveform, the envelope line and clip marks; offline media is
    /// hatched, with the envelope still drawn.
    private func drawAudio(
        _ r: EditedRegion, span: AudioEditorGeometry.FrameSpan, axis: FrameAxis, amplitude: Int, channels: Int,
        in wave: CGRect
    ) {
        // The span's own rect: its columns line up with the envelope's.
        let spanRect = CGRect(x: wave.minX + span.x, y: wave.minY, width: span.width, height: wave.height)
        guard spanRect.width > 0, spanRect.minX.isFinite else { return }
        let regionRect = AudioEditorGeometry.regionRect(r.row, axis: axis, in: wave)
        let regionVis = regionRect.intersection(wave)
        let offline = r.media == nil || r.media?.offline != nil
        let session = document.session
        var rows: OverviewRows?
        if !offline, let id = r.media?.id {
            rows = try? session.overview(media: id, fromFrame: span.from, toFrame: span.to, columns: span.columns)
            if let s = rows?.state, s != .ready, s != .coarse { rows = nil }
        }
        let envelope = try? session.regionEnvelope(
            event: r.row.id, fromFrame: span.from, toFrame: span.to, columns: span.columns)
        // 4. The file's audio outside the region, dimmed (Ruling 3): not
        // under the region's translucent fill, where it would show the
        // unshaped level around the heard one.
        NSGraphicsContext.saveGraphicsState()
        let outside = NSBezierPath(rect: wave)
        if !regionVis.isNull, regionVis.width > 0 { outside.appendRect(regionVis) }
        outside.windingRule = .evenOdd
        outside.addClip()
        if let rows {
            theme.audioWaveTrimmed.setFill()
            let file = AudioEditorGeometry.shaped(rows, envelope: nil, amplitude: amplitude)
            for b in WaveformGeometry.bars(file, in: spanRect) { b.fill() }
        }
        NSGraphicsContext.restoreGraphicsState()
        if offline {
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: wave).addClip()
            RegionDrawing.hatch(spanRect.intersection(wave), RegionPalette(theme))
            NSGraphicsContext.restoreGraphicsState()
        }
        guard !regionVis.isNull, regionVis.width > 0 else { return }
        // 5. The region's fill, in the lane's strip colour.
        let lane = document.lanes.first { $0.id == r.lane.lane }
        let tint = lane.flatMap { document.laneColour($0, theme)?.nsColor }
        theme.regionFill(tint: tint).setFill()
        regionVis.fill()
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: regionVis).addClip()
        // 6. The region's part, shaped as it sounds.
        if let rows, let envelope {
            theme.audioWave.setFill()
            let heard = AudioEditorGeometry.shaped(rows, envelope: envelope, amplitude: amplitude)
            for b in WaveformGeometry.bars(heard, in: spanRect) { b.fill() }
        }
        // 7. The envelope line, on each channel's band.
        if let envelope {
            theme.audioEnvelope.setStroke()
            for k in 0..<channels {
                let points = AudioEditorGeometry.envelopeLine(
                    envelope, amplitude: amplitude, channel: k, channels: channels, in: spanRect)
                guard let first = points.first else { continue }
                let path = NSBezierPath()
                path.move(to: first)
                for p in points.dropFirst() { path.line(to: p) }
                path.lineWidth = 1.5
                path.stroke()
            }
        }
        // 8. Clip marks along each band's top.
        if let rows, let envelope, span.columns > 0 {
            let w = spanRect.width / CGFloat(span.columns)
            let band = wave.height / CGFloat(channels)
            theme.audioClip.setFill()
            for c in AudioEditorGeometry.clipColumns(rows, envelope: envelope) {
                for k in 0..<channels {
                    CGRect(
                        x: spanRect.minX + CGFloat(c) * w, y: wave.minY + band * CGFloat(k), width: max(w, 1),
                        height: theme.spacing.xxs
                    ).fill()
                }
            }
        }
        NSGraphicsContext.restoreGraphicsState()
        theme.regionLine.setStroke()
        let outline = NSBezierPath(rect: regionVis.insetBy(dx: 0.5, dy: 0.5))
        outline.lineWidth = 1
        outline.stroke()
        if offline {
            let attrs: [NSAttributedString.Key: Any] = [
                .font: RegionDrawing.offlineFont(theme), .foregroundColor: theme.offlineColour,
            ]
            let text = NSString(string: "Offline")
            if let box = RegionDrawing.offlineLabelRect(r.row, in: regionVis, size: text.size(withAttributes: attrs)) {
                RegionDrawing.pill(text, in: box, attrs: attrs, RegionPalette(theme))
            }
        }
    }

    /// 9. The handles: the fade corners, the trim edges and the gain band;
    /// the one pressed in `audioHandleSelected`.
    private func drawHandles(_ r: EditedRegion, axis: FrameAxis, in wave: CGRect) {
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: wave).addClip()
        let radius = theme.radius.s
        for (h, rect) in AudioEditorGeometry.drawn(r.row, axis: axis, in: wave) {
            let active = pressed?.handle == h
            let colour = active ? theme.audioHandleSelected : theme.audioHandle
            switch h {
            case .fadeIn, .fadeOut:
                colour.setFill()
                NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            case .start, .end:
                colour.withAlphaComponent(active ? 0.45 : 0.18).setFill()
                NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
                colour.withAlphaComponent(active ? 1 : 0.6).setFill()
                let line = theme.spacing.xxs
                CGRect(x: h == .start ? rect.minX : rect.maxX - line, y: rect.minY, width: line, height: rect.height)
                    .fill()
            case .gain:
                colour.withAlphaComponent(active ? 0.6 : 0.25).setFill()
                NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            }
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    // MARK: Accessibility (Ruling 10)

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Audio Editor" }

    static func accessibilityLabel(_ h: AudioEditorHandle) -> String {
        switch h {
        case .fadeIn: "Fade In Handle"
        case .fadeOut: "Fade Out Handle"
        case .start: "Start Handle"
        case .end: "End Handle"
        case .gain: "Gain Handle"
        }
    }

    /// One element per handle, at its hit area (inside the wave rect).
    public override func accessibilityChildren() -> [Any]? {
        model.sync(width: wave.width)
        guard let r = document.editedRegion else {
            a11yElements = []
            return []
        }
        let items = AudioEditorGeometry.handles(r.row, axis: model.axis, in: wave).map { h, rect in
            (
                item: CanvasAccessibilityItem(
                    label: Self.accessibilityLabel(h), frame: rect, selected: false, press: nil),
                frame: rect
            )
        }
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}
