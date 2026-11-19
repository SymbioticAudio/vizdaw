import AppKit
import SymbioticTokens
import VizDAWAppModel

/// The transport bar as one row of items (groups-vcas-transport Ruling
/// 14): it lays its items out by `TransportRowFit` on every `layout()`.
/// What does not fit is hidden (never removed, so the fields view is never
/// remade) and listed in », whose menu is built from the folded items'
/// `TransportMenuEntry`s. A right-click on the row offers "Customize
/// Transport Bar…" (Task 12 wires it).
@MainActor
public final class TransportRowView: NSView, ThemedView, NSPopoverDelegate {
    /// The row's padding at each end.
    public static let padding: CGFloat = 8
    /// The status line's note when the channel clicks » (a menu tracks
    /// modally, so the channel's click opens none).
    public static let channelNote = "The » menu does not open for the control channel; use ui.set_transport_bar."

    public var theme: Theme = ThemedViews.current {
        didSet {
            themeDidChange(from: oldValue)
            if theme != oldValue {
                applyTheme()
                needsLayout = true
            }
        }
    }

    public let document: DocumentModel
    /// The document window's row shows the document-only items; the mixer
    /// window's skips them (Ruling 13).
    public let documentWindow: Bool
    public private(set) var transportLayout: TransportLayout
    /// Makes the views this module cannot (the app's own items); nil: this
    /// module's own view, or an empty one.
    private let makeView: @MainActor (TransportItem) -> NSView?
    /// The DSP load the DSP item's » entry shows.
    public var dspLoad: @MainActor () -> Float = { 0 }
    /// "Customize Transport Bar…" (Task 12); the item is off while nil.
    public var onCustomize: (@MainActor () -> Void)?

    /// One item and its view.
    public struct Slot {
        public let item: TransportItem
        public let view: NSView
    }

    public private(set) var slots: [Slot] = []
    /// The » button.
    public let chevron: NSButton
    /// The items shown and folded at the last layout, in bar order.
    public private(set) var shownItems: [TransportItem] = []
    public private(set) var foldedItems: [TransportItem] = []
    private var popover: NSPopover?
    /// The view lent to the popover, and the size it had.
    private var lent: NSView?

    public init(
        document: DocumentModel, layout: TransportLayout, documentWindow: Bool,
        makeView: @escaping @MainActor (TransportItem) -> NSView?
    ) {
        self.document = document
        self.documentWindow = documentWindow
        self.transportLayout = layout
        self.makeView = makeView
        chevron = NSButton(title: "»", target: nil, action: nil)
        super.init(frame: .zero)
        clipsToBounds = true
        chevron.bezelStyle = .push
        chevron.refusesFirstResponder = true
        chevron.setAccessibilityLabel("More Transport Items")
        chevron.cell?.setAccessibilityLabel("More Transport Items")
        chevron.toolTip = "More transport items"
        chevron.target = self
        chevron.action = #selector(chevronPressed(_:))
        chevron.isHidden = true
        addSubview(chevron)
        build()
        applyTheme()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// Shows `l`, keeping the views of items it still holds.
    public func setLayout(_ l: TransportLayout) {
        guard l != transportLayout else { return }
        transportLayout = l
        build()
    }

    /// Makes the views of the layout's items, reusing those already made
    /// (one per built-in item; spaces and separators by their place).
    private func build() {
        closePopover()
        var old: [String: NSView] = [:]
        for (n, s) in slots.enumerated() {
            old[key(s.item, n)] = s.view
        }
        var next: [Slot] = []
        for (n, item) in transportLayout.items(documentWindow: documentWindow).enumerated() {
            let k = key(item, n)
            let v = old.removeValue(forKey: k) ?? make(item)
            next.append(Slot(item: item, view: v))
        }
        for v in old.values { v.removeFromSuperview() }
        slots = next
        for s in slots where s.view.superview !== self { addSubview(s.view, positioned: .below, relativeTo: chevron) }
        TransportFieldsView.link(slots.compactMap { $0.view as? TransportFieldsView })
        passTheme(theme)
        invalidateIntrinsicContentSize()
        needsLayout = true
    }

    private func key(_ item: TransportItem, _ n: Int) -> String {
        item.id.repeats ? "\(item.token)#\(n)" : item.id.rawValue + ":" + item.size.rawValue
    }

    private func make(_ item: TransportItem) -> NSView {
        if let v = makeView(item) { return v }
        let id = item.id
        if let b = TransportItemEntries.box(id) {
            return TransportReadoutsView(document: document, boxes: [b], size: item.size)
        }
        let f = TransportItemEntries.fields(id)
        if !f.isEmpty { return TransportFieldsView(document: document, fields: f) }
        if id == .separator { return TransportSeparatorView() }
        return TransportSpaceView()
    }

    private func applyTheme() {
        appearance = NSAppearance(named: theme.appearance.nsAppearanceName)
        chevron.contentTintColor = theme.label
    }

    // MARK: Layout

    /// The row's kind of `item`.
    static func kind(_ item: TransportItem) -> TransportRowFit.Kind {
        switch item.id {
        case .cluster: .fixed
        case .space: .space
        case .separator: .separator
        default: .content
        }
    }

    /// A view's size on the row: the fields' fitting size, else its own.
    static func idealSize(_ v: NSView) -> NSSize {
        if let f = v as? TransportFieldsView { return NSSize(width: f.fittingWidth, height: f.fittingHeight) }
        if v is TransportSpaceView { return .zero }
        let s = v.intrinsicContentSize
        if s.width != NSView.noIntrinsicMetric, s.height != NSView.noIntrinsicMetric { return s }
        return v.fittingSize
    }

    /// The row's spacing between items.
    public var spacing: CGFloat { theme.spacing.m }

    private var chevronSize: NSSize { chevron.intrinsicContentSize }

    /// The slots on the row now: an item with nothing to show (no step
    /// entry summary, no faults) is neither shown nor folded.
    private func present() -> [(slot: Int, piece: TransportRowFit.Piece, size: NSSize)] {
        slots.enumerated().compactMap { n, s in
            let kind = Self.kind(s.item)
            let size = Self.idealSize(s.view)
            if (kind == .content || kind == .fixed) && !(size.width > 0) { return nil }
            return (n, TransportRowFit.Piece(width: size.width, kind: kind), size)
        }
    }

    /// The height its items and its padding need.
    public var fittingHeight: CGFloat {
        let tallest = present().map(\.size.height).filter(\.isFinite).max() ?? 0
        return ceil(max(tallest, chevronSize.height)) + 2 * theme.spacing.xs
    }

    public override var intrinsicContentSize: NSSize {
        NSSize(width: NSView.noIntrinsicMetric, height: fittingHeight)
    }

    public override func resizeSubviews(withOldSize oldSize: NSSize) {
        super.resizeSubviews(withOldSize: oldSize)
        needsLayout = true
    }

    public override func layout() {
        super.layout()
        let items = present()
        let pieces = items.map(\.piece)
        let inner = bounds.width - 2 * Self.padding
        let gap = spacing
        let chev = chevronSize
        let fit = TransportRowFit.fit(pieces, width: inner, spacing: gap, chevron: chev.width)
        let folded = fit.folded
        let shown = fit.shown
        var extra = inner - TransportRowFit.used(shown, pieces, spacing: gap)
        if !folded.isEmpty { extra -= gap + chev.width }
        let spaces = shown.filter { pieces[$0].kind == .space }
        let share = spaces.isEmpty || !(extra > 0) ? 0 : extra / CGFloat(spaces.count)
        var visible: Set<Int> = []
        var x = Self.padding
        for i in shown {
            let (slot, piece, size) = items[i]
            let w = max(TransportRowFit.clean(piece.width) + (piece.kind == .space ? share : 0), 0)
            let h = min(size.height.isFinite ? size.height : 0, bounds.height)
            let v = slots[slot].view
            v.frame = NSRect(x: x, y: (bounds.height - h) / 2, width: w, height: h)
            visible.insert(slot)
            x += w + gap
        }
        for (n, s) in slots.enumerated() where s.view !== lent {
            s.view.isHidden = !visible.contains(n)
        }
        chevron.isHidden = folded.isEmpty
        chevron.frame = NSRect(
            x: bounds.width - Self.padding - chev.width, y: (bounds.height - chev.height) / 2, width: chev.width,
            height: chev.height)
        shownItems = shown.map { slots[items[$0].slot].item }.filter { Self.kind($0) != .space }
        foldedItems = folded.map { slots[items[$0].slot].item }
    }

    // MARK: Menus

    /// The » menu: the folded items' entries, in bar order.
    public func overflowMenu() -> NSMenu {
        let entries = foldedItems.compactMap {
            TransportItemEntries.overflow($0, document, documentWindow: documentWindow, dspLoad: dspLoad())
        }
        return TransportMenus.menu(entries) { [weak self] title in self?.showPopover(titled: title) }
    }

    @objc private func chevronPressed(_ sender: NSButton) {
        guard !ControlUI.isChannel(NSApp.currentEvent) else {
            document.editor.status = Self.channelNote
            return
        }
        overflowMenu().popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 2), in: sender)
    }

    /// The row's own menu (a right-click on it, not on an item).
    public func rowMenu() -> NSMenu {
        let m = NSMenu()
        m.autoenablesItems = false
        let c = onCustomize
        m.addItem(TransportMenus.item("Customize Transport Bar…", isOn: nil, enabled: c != nil) { c?() })
        return m
    }

    public override func menu(for event: NSEvent) -> NSMenu? {
        ControlUI.isChannel(event) ? nil : rowMenu()
    }

    // MARK: Popovers

    /// Shows the folded item titled `title` in a popover under »: its own
    /// view, lent until the popover closes.
    public func showPopover(titled title: String) {
        guard let slot = slots.first(where: { foldedItems.contains($0.item) && $0.item.id.title == title }) else {
            return
        }
        closePopover()
        let v = slot.view
        let size = Self.idealSize(v)
        let pad = theme.spacing.s
        let box = NSView(frame: NSRect(x: 0, y: 0, width: size.width + 2 * pad, height: size.height + 2 * pad))
        v.removeFromSuperview()
        v.frame = NSRect(x: pad, y: pad, width: size.width, height: size.height)
        v.isHidden = false
        box.addSubview(v)
        let vc = NSViewController()
        vc.view = box
        let p = NSPopover()
        p.behavior = .transient
        p.contentViewController = vc
        p.appearance = NSAppearance(named: theme.appearance.nsAppearanceName)
        p.delegate = self
        lent = v
        popover = p
        p.show(relativeTo: chevron.bounds, of: chevron, preferredEdge: .maxY)
    }

    /// The popover's item, back on the row.
    private func closePopover() {
        if let p = popover, p.isShown { p.close() }
        returnLent()
    }

    private func returnLent() {
        popover = nil
        guard let v = lent else { return }
        lent = nil
        v.removeFromSuperview()
        addSubview(v, positioned: .below, relativeTo: chevron)
        needsLayout = true
    }

    public func popoverDidClose(_ notification: Notification) { returnLent() }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { closePopover() }
    }
}

/// A flexible space: it takes the row's spare width.
final class TransportSpaceView: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { nil }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func isAccessibilityElement() -> Bool { false }
}

/// A separator: a thin line in the theme's separator colour.
final class TransportSeparatorView: NSView, ThemedView {
    var theme: Theme = ThemedViews.current {
        didSet { themeDidChange(from: oldValue) }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { nil }

    override var intrinsicContentSize: NSSize { NSSize(width: 1, height: 18) }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func isAccessibilityElement() -> Bool { false }

    override func draw(_ dirtyRect: NSRect) {
        theme.separatorLine.setFill()
        bounds.fill()
    }
}

/// Builds `NSMenu`s from `TransportMenuEntry`s (the », Click's own menu).
@MainActor
public enum TransportMenus {
    /// Holds an item's action: the item keeps it (`representedObject`), as
    /// its target is weak.
    @MainActor final class Runner: NSObject {
        let run: @MainActor () -> Void

        init(_ run: @escaping @MainActor () -> Void) { self.run = run }

        @objc func fire(_ sender: Any?) { run() }
    }

    /// One action item.
    public static func item(_ title: String, isOn: Bool?, enabled: Bool, run: @escaping @MainActor () -> Void)
        -> NSMenuItem
    {
        let r = Runner(run)
        let i = NSMenuItem(title: title, action: #selector(Runner.fire(_:)), keyEquivalent: "")
        i.target = r
        i.representedObject = r
        i.isEnabled = enabled
        if let isOn { i.state = isOn ? .on : .off }
        return i
    }

    /// The menu of `entries`; a popover entry calls `popover` with its title.
    public static func menu(_ entries: [TransportMenuEntry], popover: @escaping @MainActor (String) -> Void)
        -> NSMenu
    {
        let m = NSMenu()
        m.autoenablesItems = false
        for e in entries {
            switch e {
            case .action(let title, let isOn, let enabled, let run):
                m.addItem(item(title, isOn: isOn, enabled: enabled, run: run))
            case .text(let t):
                let i = NSMenuItem(title: t, action: nil, keyEquivalent: "")
                i.isEnabled = false
                m.addItem(i)
            case .popover(let title):
                m.addItem(item(title, isOn: nil, enabled: true) { popover(title) })
            case .submenu(let title, let isOn, let enabled, let entries):
                let i = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                i.isEnabled = enabled
                if let isOn { i.state = isOn ? .on : .off }
                i.submenu = menu(entries, popover: popover)
                m.addItem(i)
            }
        }
        return m
    }
}
