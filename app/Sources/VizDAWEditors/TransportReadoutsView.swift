import AppKit
import Observation
import SymbioticTokens
import VizDAWAppModel

/// The transport bar's LCD readouts (plan 7d Ruling 6): Bar, Time, Tempo
/// and Sig, a label over a value, drawn in the theme's readout look. Each
/// box is an accessibility element named "Bar Readout" and so on, with its
/// text as the value, so the control channel's tree reads them. Each keeps
/// a fixed width (the widest text in its font), so digits never jitter the
/// bar; a longer text is cut at the tail with "…".
@MainActor
public final class TransportReadoutsView: NSView, ThemedView {
    /// One box: its label and its value.
    final class BoxView: NSView {
        let box: TransportReadouts.Box
        let title = NSTextField(labelWithString: "")
        let value = NSTextField(labelWithString: "")

        init(_ box: TransportReadouts.Box) {
            self.box = box
            super.init(frame: .zero)
            for f in [title, value] {
                f.maximumNumberOfLines = 1
                f.cell?.usesSingleLineMode = true
                f.refusesFirstResponder = true
                f.isSelectable = false
                f.setAccessibilityElement(false)
                addSubview(f)
            }
            value.lineBreakMode = .byTruncatingTail
            title.lineBreakMode = .byClipping
            setAccessibilityElement(true)
            setAccessibilityRole(.staticText)
            setAccessibilityLabel(box.accessibilityLabel)
        }

        required init?(coder: NSCoder) { nil }

        override var isFlipped: Bool { true }
    }

    /// The theme it draws in (plan 7b Ruling 12): its host, or the themed
    /// view above it, sets it.
    public var theme: Theme = ThemedViews.current {
        didSet {
            themeDidChange(from: oldValue)
            applyTheme()
        }
    }

    public let document: DocumentModel
    private(set) var boxes: [TransportReadouts.Box: BoxView] = [:]
    /// Each box's width, for the theme's fonts.
    private var widths: [TransportReadouts.Box: CGFloat] = [:]
    private var rowHeight: CGFloat = 0

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        for b in TransportReadouts.Box.allCases {
            let v = BoxView(b)
            boxes[b] = v
            addSubview(v)
        }
        applyTheme()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// The value field of `box`.
    public func valueField(_ box: TransportReadouts.Box) -> NSTextField? { boxes[box]?.value }

    /// The width of `box`'s value: its widest text in the value font.
    public func valueWidth(_ box: TransportReadouts.Box) -> CGFloat {
        guard let v = boxes[box]?.value else { return 0 }
        return measure(TransportReadouts.widest.text(box), v.font)
    }

    /// A text's width in `font`, with a little room each side so the label
    /// never cuts a text that is exactly as wide as the widest.
    private func measure(_ text: String, _ font: NSFont?) -> CGFloat {
        let f = NSTextField(labelWithString: text)
        f.font = font
        return ceil(f.intrinsicContentSize.width) + 2 * theme.spacing.xxs
    }

    /// The edge inside the group, where it has one.
    private var inset: CGFloat { TransportReadouts.look(theme).edge == nil ? 0 : TransportReadouts.look(theme).gap }

    /// Fonts and colours from the theme; widths measured once per theme.
    private func applyTheme() {
        let t = theme
        let look = TransportReadouts.look(t)
        appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        let digits = t.digitsFont(t.type.title, weight: t.type.title.weight.appKit)
        let caption = t.font(t.type.caption, weight: t.type.caption.weight.appKit)
        var tallest: CGFloat = 0
        for (b, v) in boxes {
            v.value.font = digits
            v.value.textColor = look.value.nsColor
            v.title.font = caption
            v.title.textColor = look.label.nsColor
            v.title.stringValue = TransportReadouts.label(b.title, t)
            let inner = max(measure(TransportReadouts.widest.text(b), digits), ceil(v.title.intrinsicContentSize.width))
            widths[b] = inner + 2 * look.padding
            tallest = max(tallest, ceil(v.title.intrinsicContentSize.height + v.value.intrinsicContentSize.height))
        }
        rowHeight = tallest
        refresh()
        invalidateIntrinsicContentSize()
        needsLayout = true
        needsDisplay = true
    }

    private func observe() {
        withObservationTracking {
            _ = document.transport
            _ = document.snapshot
            refresh()
        } onChange: { [weak self] in
            DispatchQueue.main.async { self?.observe() }
        }
    }

    /// Shows the document's texts.
    func refresh() {
        let texts = document.readoutTexts
        for (b, v) in boxes {
            let text = texts.text(b)
            if v.value.stringValue != text { v.value.stringValue = text }
            v.setAccessibilityValue(text)
        }
    }

    public override var intrinsicContentSize: NSSize {
        let look = TransportReadouts.look(theme)
        let n = CGFloat(boxes.count)
        let w = widths.values.reduce(0, +) + look.gap * max(n - 1, 0) + 2 * inset
        return NSSize(width: w, height: rowHeight + 2 * inset)
    }

    public override func layout() {
        super.layout()
        let look = TransportReadouts.look(theme)
        var x = inset
        for b in TransportReadouts.Box.allCases {
            guard let v = boxes[b], let w = widths[b] else { continue }
            v.frame = NSRect(x: x, y: inset, width: w, height: max(bounds.height - 2 * inset, 0))
            let th = v.title.intrinsicContentSize.height
            let vh = v.value.intrinsicContentSize.height
            let top = max((v.bounds.height - th - vh) / 2, 0)
            v.title.frame = NSRect(x: look.padding, y: top, width: max(w - 2 * look.padding, 0), height: th)
            v.value.frame = NSRect(x: look.padding, y: top + th, width: valueFieldWidth(b, in: w, look), height: vh)
            x += w + look.gap
        }
    }

    /// The value field is the widest text's width, or the box's inner width
    /// if that is less (a label wider than the digits).
    private func valueFieldWidth(_ b: TransportReadouts.Box, in box: CGFloat, _ look: TransportReadouts.ReadoutLook)
        -> CGFloat
    {
        min(valueWidth(b), max(box - 2 * look.padding, 0))
    }

    public override func draw(_ dirtyRect: NSRect) {
        let look = TransportReadouts.look(theme)
        let frames = TransportReadouts.Box.allCases.compactMap { boxes[$0]?.frame }
        if let edge = look.edge {
            let group = NSBezierPath(roundedRect: bounds, xRadius: look.radius, yRadius: look.radius)
            edge.nsColor.setFill()
            group.fill()
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(
                roundedRect: bounds.insetBy(dx: inset, dy: inset), xRadius: max(look.radius - inset, 0),
                yRadius: max(look.radius - inset, 0)
            ).addClip()
            look.fill.nsColor.setFill()
            for f in frames { f.fill() }
            NSGraphicsContext.restoreGraphicsState()
        } else {
            look.fill.nsColor.setFill()
            for f in frames { NSBezierPath(roundedRect: f, xRadius: look.radius, yRadius: look.radius).fill() }
        }
    }
}
