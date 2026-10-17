import AppKit
import QuartzCore
import VizDAWAppModel
import VizDAWCore

/// A layer-backed, flipped view that draws through a closure and lets
/// every event through to its canvas.
public final class CanvasLayerView: NSView {
    var paint: (@MainActor (CGRect) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var isOpaque: Bool { false }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }
    public override func draw(_ dirtyRect: NSRect) { paint?(dirtyRect) }
}

/// One element an editor exposes to VoiceOver (spec §3.5).
public struct CanvasAccessibilityItem {
    public let label: String
    /// In `content` coordinates.
    public let frame: CGRect
    public let selected: Bool
    public let press: (@MainActor () -> Void)?

    public init(label: String, frame: CGRect, selected: Bool, press: (@MainActor () -> Void)?) {
        self.label = label
        self.frame = frame
        self.selected = selected
        self.press = press
    }
}

@MainActor
final class CanvasElement: NSAccessibilityElement {
    private let press: (@MainActor () -> Void)?

    init(_ item: CanvasAccessibilityItem, parent: NSView, frame: CGRect) {
        press = item.press
        super.init()
        setAccessibilityRole(.button)
        setAccessibilityLabel(item.label)
        setAccessibilityParent(parent)
        setAccessibilityFrameInParentSpace(frame)
        setAccessibilitySelected(item.selected)
    }

    override func accessibilityPerformPress() -> Bool {
        guard let press else { return false }
        press()
        return true
    }
}

/// The shared editor surface (spec §3.5): a ruler on top and, below it,
/// grid, content and overlay layers (each a layer-backed view, redrawn
/// only for its dirty rect; the grid only when the axis or size changes)
/// plus a playhead layer moved — not redrawn — on every display frame.
/// Subclasses draw content and handle content gestures; the canvas owns
/// scrolling, zoom, the ruler, auto-scroll, snap toggling, the playhead
/// and the accessibility container.
@MainActor
open class EditorCanvas: NSView {
    public static let rulerHeight: CGFloat = 22

    public let grid = CanvasLayerView()
    public let content = CanvasLayerView()
    public let overlay = CanvasLayerView()
    public let ruler = CanvasLayerView()
    public let playheadLayer = CALayer()

    public var axis = TimeAxis() {
        didSet {
            guard axis != oldValue else { return }
            reload()
            onAxisChange?(axis)
            movePlayhead(now: Self.hostNow())
        }
    }

    public var verticalOffset: CGFloat = 0 {
        didSet {
            guard verticalOffset != oldValue else { return }
            content.needsDisplay = true
            overlay.needsDisplay = true
            onVerticalScroll?(verticalOffset)
        }
    }

    /// Follow playback (page mode).
    public var follow = true
    /// The range a ruler drag is selecting.
    public private(set) var rulerPreview: TickRange?

    public var playheadSource: (@MainActor (UInt64) -> Double?)?
    public var onRulerRange: ((TickRange) -> Void)?
    public var onRulerClick: ((Int64) -> Void)?
    public var onToggleSnap: (() -> Void)?
    public var onAxisChange: ((TimeAxis) -> Void)?
    public var onVerticalScroll: ((CGFloat) -> Void)?

    private var link: CADisplayLink?
    private var playing = false
    private var rulerAnchor: Int64?
    private var rulerMoved = false
    private var dragPoint: CGPoint?
    private var dragModifiers: Modifiers = []
    private var scrollTimer: Timer?

    public override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setUp()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func setUp() {
        wantsLayer = true
        layer?.backgroundColor = NSColor.textBackgroundColor.cgColor
        for v in [grid, content, overlay, ruler] { addSubview(v) }
        grid.paint = { [unowned self] r in self.drawGrid(r) }
        content.paint = { [unowned self] r in self.drawContent(in: r) }
        overlay.paint = { [unowned self] r in self.drawOverlayBase(r) }
        ruler.paint = { [unowned self] r in self.drawRuler(r) }
        playheadLayer.backgroundColor = NSColor.systemRed.cgColor
        playheadLayer.isHidden = true
        overlay.layer?.addSublayer(playheadLayer)
    }

    // MARK: Subclass surface

    /// The meter map of what is shown (grid, ruler, snap).
    open var meter: [MeterEntryRow] { [] }
    open var snap: Snap { Snap() }
    /// The selection's time range, shaded in the ruler and the content.
    open var selectedRange: TickRange? { nil }
    /// Height of everything below the ruler (vertical scroll limit).
    open var contentHeight: CGFloat { content.bounds.height }
    open var accessibilityName: String { "Editor" }

    open func drawContent(in dirty: CGRect) {}
    open func drawOverlay(in dirty: CGRect) {}
    open func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {}
    open func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {}
    open func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {}
    open func contentCancel() {}
    open func accessibilityItems() -> [CanvasAccessibilityItem] { [] }

    /// Everything redraws (data changed).
    public func reload() {
        for v in [grid, content, overlay, ruler] { v.needsDisplay = true }
    }

    public static func hostNow() -> UInt64 { UInt64(max(CACurrentMediaTime(), 0) * 1e9) }

    // MARK: Layout

    open override func layout() {
        super.layout()
        let below = NSRect(
            x: 0, y: Self.rulerHeight, width: bounds.width,
            height: max(bounds.height - Self.rulerHeight, 0))
        grid.frame = below
        content.frame = below
        overlay.frame = below
        ruler.frame = NSRect(x: 0, y: 0, width: bounds.width, height: Self.rulerHeight)
        verticalOffset = clampVertical(verticalOffset)
        movePlayhead(now: Self.hostNow())
    }

    func clampVertical(_ y: CGFloat) -> CGFloat {
        min(max(y, 0), max(contentHeight - content.bounds.height, 0))
    }

    // MARK: Drawing

    private func drawGrid(_ dirty: CGRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let lines = Grid.lines(
            meter: meter, from: axis.tick(atX: dirty.minX - 1), to: axis.tick(atX: dirty.maxX + 1),
            ticksPerPoint: axis.ticksPerPoint)
        for line in lines {
            let x = axis.x(line.tick).rounded() + 0.5
            switch line.kind {
            case .bar: ctx.setStrokeColor(NSColor.separatorColor.cgColor)
            case .beat: ctx.setStrokeColor(NSColor.separatorColor.withAlphaComponent(0.35).cgColor)
            }
            ctx.setLineWidth(1)
            ctx.move(to: CGPoint(x: x, y: dirty.minY))
            ctx.addLine(to: CGPoint(x: x, y: dirty.maxY))
            ctx.strokePath()
        }
    }

    private func shade(_ range: TickRange?, in rect: CGRect, alpha: CGFloat) {
        guard let range else { return }
        let x0 = axis.x(range.start)
        let x1 = axis.x(range.end)
        NSColor.controlAccentColor.withAlphaComponent(alpha).setFill()
        CGRect(x: x0, y: rect.minY, width: max(x1 - x0, 1), height: rect.height).fill()
    }

    private func drawOverlayBase(_ dirty: CGRect) {
        shade(rulerPreview ?? selectedRange, in: overlay.bounds, alpha: 0.10)
        drawOverlay(in: dirty)
    }

    private func drawRuler(_ dirty: CGRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        shade(rulerPreview ?? selectedRange, in: ruler.bounds, alpha: 0.35)
        let first = meter.first ?? MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)
        let every = Grid.labelEvery(
            barTicks: MeterMath.barTicks(first), ticksPerPoint: axis.ticksPerPoint)
        let lines = Grid.lines(
            meter: meter, from: axis.tick(atX: dirty.minX - 60), to: axis.tick(atX: dirty.maxX),
            ticksPerPoint: axis.ticksPerPoint)
        let h = ruler.bounds.height
        let font = NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular)
        ctx.setStrokeColor(NSColor.secondaryLabelColor.cgColor)
        ctx.setLineWidth(1)
        for line in lines {
            let x = axis.x(line.tick).rounded() + 0.5
            switch line.kind {
            case .bar(let n):
                ctx.move(to: CGPoint(x: x, y: h * 0.35))
                ctx.addLine(to: CGPoint(x: x, y: h))
                ctx.strokePath()
                if MeterMath.floorDiv(n - 1, every) * every == n - 1 {
                    NSString(string: "\(n)").draw(
                        at: CGPoint(x: x + 3, y: 2),
                        withAttributes: [.font: font, .foregroundColor: NSColor.labelColor])
                }
            case .beat:
                ctx.move(to: CGPoint(x: x, y: h * 0.7))
                ctx.addLine(to: CGPoint(x: x, y: h))
                ctx.strokePath()
            }
        }
        NSColor.separatorColor.setFill()
        CGRect(x: dirty.minX, y: h - 1, width: dirty.width, height: 1).fill()
    }

    // MARK: Playhead (spec §3.4)

    /// Starts or stops the display link; also moves the playhead once. The
    /// link runs only while the canvas is in a window (it retains its target).
    public func setPlaying(_ on: Bool) {
        playing = on
        updateLink()
        movePlayhead(now: Self.hostNow())
    }

    /// Whether the per-frame display link is running.
    var isLinkRunning: Bool { link != nil }

    private func updateLink() {
        if playing, window != nil {
            guard link == nil else { return }
            let l = displayLink(target: self, selector: #selector(frameTick(_:)))
            l.add(to: .main, forMode: .common)
            link = l
        } else {
            link?.invalidate()
            link = nil
        }
    }

    open override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateLink()
    }

    @objc private func frameTick(_ l: CADisplayLink) {
        movePlayhead(now: UInt64(max(l.timestamp, 0) * 1e9))
    }

    func movePlayhead(now: UInt64) {
        guard let tick = playheadSource?(now) else {
            playheadLayer.isHidden = true
            return
        }
        if playing, follow,
            let origin = PageFollow.origin(forPlayhead: tick, axis: axis, width: content.bounds.width)
        {
            axis = axis.scrolled(toTick: origin)
        }
        let x = CGFloat((tick - axis.originTick) / axis.ticksPerPoint).rounded()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        playheadLayer.isHidden = x < -1 || x > overlay.bounds.width + 1
        playheadLayer.frame = CGRect(x: x, y: 0, width: 1, height: overlay.bounds.height)
        CATransaction.commit()
    }

    // MARK: Events

    open override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = convert(e.locationInWindow, from: nil)
        dragModifiers = Modifiers(event: e)
        if p.y < Self.rulerHeight {
            rulerAnchor = axis.tick(atX: p.x)
            rulerMoved = false
            return
        }
        contentMouseDown(
            content.convert(e.locationInWindow, from: nil), clicks: e.clickCount,
            modifiers: dragModifiers)
    }

    open override func mouseDragged(with e: NSEvent) {
        dragPoint = convert(e.locationInWindow, from: nil)
        dragModifiers = Modifiers(event: e)
        startAutoScroll()
        dragStep()
    }

    open override func mouseUp(with e: NSEvent) {
        stopAutoScroll()
        dragPoint = nil
        if let anchor = rulerAnchor {
            rulerAnchor = nil
            let range = rulerPreview
            rulerPreview = nil
            reload()
            if rulerMoved, let range {
                onRulerRange?(range)
            } else if !rulerMoved {
                onRulerClick?(snap.apply(anchor, meter: meter))
            }
            return
        }
        contentMouseUp(content.convert(e.locationInWindow, from: nil), modifiers: Modifiers(event: e))
    }

    /// One drag step at the last pointer position (also run by auto-scroll,
    /// so content under a still pointer follows the scrolled axis).
    private func dragStep() {
        guard let p = dragPoint else { return }
        if let anchor = rulerAnchor {
            rulerMoved = true
            rulerPreview = RulerSelection.range(
                fromTick: anchor, toX: p.x, axis: axis, snap: snap, meter: meter)
            ruler.needsDisplay = true
            overlay.needsDisplay = true
            return
        }
        contentMouseDragged(convert(p, to: content), modifiers: dragModifiers)
    }

    private func startAutoScroll() {
        guard scrollTimer == nil else { return }
        scrollTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.autoScrollStep() }
        }
    }

    private func stopAutoScroll() {
        scrollTimer?.invalidate()
        scrollTimer = nil
    }

    private func autoScrollStep() {
        guard let p = dragPoint else { return }
        let v = AutoScroll.speed(x: p.x, width: bounds.width)
        guard v != 0 else { return }
        axis = axis.scrolled(byPoints: v / 60)
        dragStep()
    }

    open override func keyDown(with e: NSEvent) {
        let plain = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
            .subtracting([.capsLock, .numericPad, .function]).isEmpty
        if e.keyCode == 53 {  // Escape
            cancelGestures()
        } else if plain, e.charactersIgnoringModifiers?.lowercased() == "g" {
            onToggleSnap?()
        } else {
            super.keyDown(with: e)
        }
    }

    open override func cancelOperation(_ sender: Any?) { cancelGestures() }

    private func cancelGestures() {
        stopAutoScroll()
        if rulerAnchor != nil {
            rulerAnchor = nil
            rulerPreview = nil
            reload()
        }
        contentCancel()
    }

    open override func scrollWheel(with e: NSEvent) {
        let x = convert(e.locationInWindow, from: nil).x
        if e.modifierFlags.contains(.command) {
            axis = axis.zoomed(by: pow(1.01, Double(e.scrollingDeltaY)), aroundX: x)
            return
        }
        axis = axis.scrolled(byPoints: -e.scrollingDeltaX)
        verticalOffset = clampVertical(verticalOffset - e.scrollingDeltaY)
    }

    open override func magnify(with e: NSEvent) {
        axis = axis.zoomed(by: 1 + Double(e.magnification), aroundX: convert(e.locationInWindow, from: nil).x)
    }

    // MARK: Accessibility (spec §3.5)

    open override func isAccessibilityElement() -> Bool { true }
    open override func accessibilityRole() -> NSAccessibility.Role? { .group }
    open override func accessibilityLabel() -> String? { accessibilityName }

    open override func accessibilityChildren() -> [Any]? {
        accessibilityItems().map { item in
            CanvasElement(item, parent: self, frame: content.convert(item.frame, to: self))
        }
    }
}
