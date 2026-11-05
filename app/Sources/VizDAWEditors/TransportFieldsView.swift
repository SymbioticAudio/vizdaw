import AppKit
import Observation
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The transport bar's fields (parent spec §9, plan 4f Ruling 16):
/// Position, Edit In and Out, Punch In and Out, each a text field showing
/// bar.beat.unit. A key (P, E, U) focuses one; Return, Tab and Shift-Tab
/// commit the typed position; Escape, a click elsewhere and the window
/// losing key cancel it (plan 7d Ruling 7). Each field is an accessibility
/// element named by its title, so `vizctl find Position` reaches it.
@MainActor
public final class TransportFieldsView: NSView, NSTextFieldDelegate, ThemedView {
    /// The theme it draws in (plan 7b Ruling 12): its host, or the themed
    /// view above it, sets it; a change redraws it, the themed views below
    /// and its labels' and fields' colours and fonts.
    public var theme: Theme = ThemedViews.current {
        didSet {
            themeDidChange(from: oldValue)
            applyTheme()
        }
    }

    /// The text-editing owner while a field has the keyboard.
    public static let textOwner = "transport"
    public let document: DocumentModel
    public private(set) var fields: [TransportField: NSTextField] = [:]
    /// Each field's title label.
    public private(set) var labels: [TransportField: NSTextField] = [:]
    /// The field being typed in.
    private(set) var editing: TransportField?
    private let stack = NSStackView()
    /// While a field edits: the mouse-down monitor and the window's
    /// resign-key observer that end the edit (plan 7d pre-flight C1).
    private var mouseMonitor: Any?
    private var resignObserver: NSObjectProtocol?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        // Narrower than its fitting width, the fields give (not the edge).
        stack.setClippingResistancePriority(.defaultLow, for: .horizontal)
        for f in TransportField.allCases {
            let label = NSTextField(labelWithString: f.title)
            label.setContentCompressionResistancePriority(.required, for: .horizontal)
            labels[f] = label
            let field = TransportTextField(string: "")
            field.alignment = .right
            field.delegate = self
            field.title = f.title
            field.identifier = NSUserInterfaceItemIdentifier("transport.\(f.rawValue)")
            // 86 points, narrowing to 72 in a row too narrow for all five
            // (Task 7 review I1); `fittingWidth` reports the 86.
            let preferred = field.widthAnchor.constraint(equalToConstant: Self.fieldWidth)
            preferred.priority = .defaultLow
            preferred.isActive = true
            field.widthAnchor.constraint(greaterThanOrEqualToConstant: Self.narrowFieldWidth).isActive = true
            field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            fields[f] = field
            stack.addArrangedSubview(label)
            stack.addArrangedSubview(field)
            if f == .position || f == .editOut { stack.setCustomSpacing(16, after: field) }
        }
        // Tab and Shift-Tab go round the fields in order (Edit Out and
        // Punch Out have no key of their own; Ruling 16, final review I-2).
        let order = TransportField.allCases.compactMap { fields[$0] }
        for (i, field) in order.enumerated() { field.nextKeyView = order[(i + 1) % order.count] }
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            // The last field stays inside: the fields narrow before it is cut.
            (order.last ?? stack).trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        applyTheme()
        observe()
    }

    /// The labels' and fields' text in the theme's own colours (never the
    /// window's faint defaults: the B4 run found them faint in light),
    /// on the theme's canvas, in its type sizes.
    private func applyTheme() {
        let t = theme
        appearance = NSAppearance(named: t.appearance.nsAppearanceName)
        for label in labels.values {
            label.font = t.font(t.type.label)
            label.textColor = t.label
        }
        for field in fields.values {
            field.font = t.digitsFont(t.type.body)
            field.textColor = t.label
            field.drawsBackground = true
            field.backgroundColor = t.canvas
        }
    }

    /// The width the five fields and their labels need (the transport bar
    /// puts them on a row of their own when its row cannot give it).
    public var fittingWidth: CGFloat { stack.fittingSize.width }

    /// A field's width, and the least it narrows to.
    static let fieldWidth: CGFloat = 86
    static let narrowFieldWidth: CGFloat = 72

    /// The least width the fields take: the labels, the five fields at 72
    /// and the spacing (the bar's flow keeps the fields at least this wide).
    public var minimumWidth: CGFloat {
        fittingWidth - CGFloat(fields.count) * (Self.fieldWidth - Self.narrowFieldWidth)
    }

    public required init?(coder: NSCoder) { nil }

    private func observe() {
        withObservationTracking {
            refresh()
        } onChange: { [weak self] in
            DispatchQueue.main.async { self?.observe() }
        }
    }

    /// Every field without the keyboard shows its position. All five
    /// sources are read, so the observation tracks them whichever field
    /// is being edited.
    func refresh() {
        let texts = TransportField.allCases.map { ($0, document.fieldText($0)) }
        for (f, text) in texts {
            guard let field = fields[f], f != editing, field.currentEditor() == nil else { continue }
            field.stringValue = text
        }
    }

    /// Gives `f` the keyboard with its text selected (the P, E and U keys).
    @discardableResult
    public func focus(_ f: TransportField) -> Bool {
        guard let field = fields[f] as? TransportTextField, let w = window else { return false }
        field.armed = true
        guard w.makeFirstResponder(field) else { return false }
        field.currentEditor()?.selectAll(nil)
        return true
    }

    func field(of obj: Notification) -> TransportField? {
        fields.first { $0.value === obj.object as? NSTextField }?.key
    }

    /// Ruling 7: an edit that ends with Return, Tab or Shift-Tab commits;
    /// any other end (a click elsewhere, the window losing key, Escape,
    /// teardown) cancels.
    static func commits(_ m: NSTextMovement?) -> Bool {
        m == .return || m == .tab || m == .backtab
    }

    /// A press while a field edits ends the edit unless it is in that
    /// field (pre-flight C1: a press on a bar button, the readouts or the
    /// bar's background never takes the keyboard, so nothing else would).
    static func endsOnMouseDown(inField: Bool) -> Bool { !inField }

    public func controlTextDidBeginEditing(_ obj: Notification) {
        editing = field(of: obj)
        document.editor.beginTextEditing(Self.textOwner)
        watchForTheEditsEnd()
        reportPunchText()
    }

    /// Each change to Punch In or Out goes to the record model, so Record
    /// waits for it on every path (fix round 1 M4).
    public func controlTextDidChange(_ obj: Notification) { reportPunchText() }

    private func reportPunchText() {
        guard let f = editing, let field = fields[f] else { return }
        document.record.barPunchTyped(f, field.stringValue)
    }

    /// The edit's mouse-down monitor or resign-key observer is installed.
    var isWatching: Bool { mouseMonitor != nil || resignObserver != nil }

    /// Installs the mouse-down monitor and the resign-key observer while a
    /// field edits; `stopWatching()` removes them when the edit ends.
    private func watchForTheEditsEnd() {
        guard mouseMonitor == nil, let w = window else { return }
        // A `@MainActor` final class is Sendable: the blocks capture it
        // strongly, and the monitor is removed when the edit ends.
        let me = self
        mouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { e in
            // Mouse events arrive on the main thread.
            nonisolated(unsafe) let event = e
            MainActor.assumeIsolated { me.handleMouseDown(event) }
            return e
        }
        resignObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didResignKeyNotification, object: w, queue: .main
        ) { _ in
            MainActor.assumeIsolated { me.resignKeyCancels() }
        }
    }

    private func stopWatching() {
        document.record.barPunchEnded()
        if let m = mouseMonitor { NSEvent.removeMonitor(m) }
        mouseMonitor = nil
        if let o = resignObserver { NotificationCenter.default.removeObserver(o) }
        resignObserver = nil
    }

    /// The monitor's seam: a press outside the editing field ends the
    /// edit (movement `.other`: it cancels), then the press goes on to
    /// what it hit, so a bar button acts (controller ruling I-4). A press
    /// on Record while Punch In or Out holds a typed time keeps the edit,
    /// so Record refuses and Return still sets it (fix round 1 M4).
    func handleMouseDown(_ event: NSEvent) {
        guard let f = editing, let field = fields[f], let w = window else { return }
        let inField = event.window === w && field.bounds.contains(field.convert(event.locationInWindow, from: nil))
        guard Self.endsOnMouseDown(inField: inField) else { return }
        reportPunchText()
        guard
            RecordModel.closeAllowed(
                pressOnRecord: RecordButtons.contains(event), draftOpen: document.record.hasOpenPunchDraft)
        else { return }
        endTheEdit(f)
    }

    /// The observer's seam: the window losing key cancels the edit.
    func resignKeyCancels() {
        guard let f = editing else { return }
        endTheEdit(f)
    }

    /// Ends `f`'s edit as a click elsewhere does: the field editor
    /// resigns with movement `.other`, which cancels.
    private func endTheEdit(_ f: TransportField) {
        if let w = window, let editor = fields[f]?.currentEditor(), w.firstResponder === editor {
            w.makeFirstResponder(nil)
        }
        // Not ended by the field editor (it had already resigned): end it here.
        if editing == f { cancel(f) }
    }

    /// Drops `f`'s typed text: it shows the value in use, and the keys
    /// work again.
    private func cancel(_ f: TransportField) {
        editing = nil
        stopWatching()
        document.editor.endTextEditing(Self.textOwner)
        (fields[f] as? TransportTextField)?.armed = false
        fields[f]?.stringValue = document.fieldText(f)
        refresh()
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let f = field(of: obj) ?? editing, let text = fields[f]?.stringValue else { return }
        editing = nil
        stopWatching()
        document.editor.endTextEditing(Self.textOwner)
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        if Self.commits(movement) {
            if text != document.fieldText(f) { document.commitField(f, text) }
        } else {
            // Ruling 7: the field shows the value in use again.
            fields[f]?.stringValue = document.fieldText(f)
        }
        (fields[f] as? TransportTextField)?.armed = false
        refresh()
        // Enter commits and gives the keyboard back to the keys. Next
        // turn: the field selects its text again after this notification.
        if movement == .return {
            DispatchQueue.main.async { [weak self] in self?.giveKeyboardBack(f) }
        }
        // Tab and Shift-Tab move on: AppKit selects the next key view
        // after this notification, so arm that field now (a field takes the
        // keyboard only when armed); disarm it next turn if it did not.
        if movement == .tab || movement == .backtab {
            let all = TransportField.allCases
            let i = all.firstIndex(of: f) ?? 0
            let next = all[(i + (movement == .tab ? 1 : all.count - 1)) % all.count]
            guard let field = fields[next] as? TransportTextField else { return }
            field.armed = true
            DispatchQueue.main.async {
                if field.currentEditor() == nil { field.armed = false }
            }
        }
    }

    /// The window's keyboard goes back to the window if `f` still has it.
    func giveKeyboardBack(_ f: TransportField) {
        guard let w = window, let field = fields[f], let editor = field.currentEditor(),
            w.firstResponder === editor
        else { return }
        w.makeFirstResponder(nil)
    }

    /// Escape reverts the field and gives the keyboard back.
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)),
            let f = fields.first(where: { $0.value === control })?.key
        else { return false }
        editing = nil
        stopWatching()
        document.editor.endTextEditing(Self.textOwner)
        fields[f]?.abortEditing()
        (fields[f] as? TransportTextField)?.armed = false
        refresh()
        window?.makeFirstResponder(nil)
        return true
    }

    /// The fields view inside `view`, if any (the P, E and U keys find it).
    public static func find(in view: NSView?) -> TransportFieldsView? {
        guard let view else { return nil }
        if let v = view as? TransportFieldsView { return v }
        for s in view.subviews {
            if let v = find(in: s) { return v }
        }
        return nil
    }

    /// Torn down mid-edit: give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil {
            stopWatching()
            if editing != nil {
                editing = nil
                document.editor.endTextEditing(Self.textOwner)
            }
        }
    }
}

/// A transport field takes the keyboard only when clicked or focused by
/// its key, never as the window's first text field when it opens, so the
/// transport and step keys work from the start (plan 4f Ruling 16).
final class TransportTextField: NSTextField, ThemedView {
    /// The theme it draws in (plan 7b Ruling 12): its host, or the themed
    /// view above it, sets it; a change redraws it and the themed views below.
    public var theme: Theme = ThemedViews.current {
        didSet { themeDidChange(from: oldValue) }
    }

    var armed = false
    /// Its name for VoiceOver and the control channel ("Position").
    var title = "" {
        didSet {
            setAccessibilityLabel(title)
            cell?.setAccessibilityLabel(title)
        }
    }

    override func accessibilityLabel() -> String? { title }

    /// A click the control channel sends is never a window-activating
    /// first click (control spec §5.5).
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    override func mouseDown(with event: NSEvent) {
        armed = true
        super.mouseDown(with: event)
    }

    override func becomeFirstResponder() -> Bool {
        guard armed else { return false }
        return super.becomeFirstResponder()
    }

    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        armed = false
    }
}

/// A view that takes every key while it records one (the Keys settings'
/// recorder): the key bindings leave its keys alone (plan 4f Ruling 17).
@MainActor
public protocol KeyCapturing: AnyObject {}

/// Records one key for a binding (plan 4f Ruling 17): press it (or press
/// its button with VoiceOver), then press the key; Escape cancels.
@MainActor
public final class KeyRecorderView: NSView, KeyCapturing, ThemedView {
    /// The theme it draws in (plan 7b Ruling 12): its host, or the themed
    /// view above it, sets it; a change redraws it and the themed views below.
    public var theme: Theme = ThemedViews.current {
        didSet { themeDidChange(from: oldValue) }
    }

    /// The command whose key is recorded (its title names the button).
    public var title = ""
    /// Called with the recorded key's chord text (`⇧⌘K`).
    public var onKey: ((String) -> Void)?
    public private(set) var recording = false {
        didSet { needsDisplay = true }
    }

    public override var acceptsFirstResponder: Bool { true }

    public override func mouseDown(with event: NSEvent) { start() }

    public func start() {
        recording = true
        window?.makeFirstResponder(self)
    }

    public override func resignFirstResponder() -> Bool {
        recording = false
        return super.resignFirstResponder()
    }

    /// Takes the key before the menus do (⌘ keys included).
    public override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard recording else { return super.performKeyEquivalent(with: event) }
        take(event)
        return true
    }

    public override func keyDown(with event: NSEvent) {
        if recording { take(event) } else { super.keyDown(with: event) }
    }

    /// Records `event`'s key (Escape alone cancels); a key VizDAW cannot
    /// bind is ignored and recording goes on.
    func take(_ event: NSEvent) {
        let f = event.modifierFlags
        let m = ModifiersRow(
            shift: f.contains(.shift), control: f.contains(.control), option: f.contains(.option),
            command: f.contains(.command))
        if event.keyCode == 53, !(m.shift || m.control || m.option || m.command) {
            recording = false
            return
        }
        guard let text = chordText(keyCode: event.keyCode, modifiers: m) else { return }
        recording = false
        onKey?(text)
    }

    public override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 1, dy: 1)
        (recording ? theme.recordingRing : theme.separatorLine).setStroke()
        let p = NSBezierPath(roundedRect: r, xRadius: theme.radius.s, yRadius: theme.radius.s)
        p.stroke()
        let s = recording ? "Press a key…" : "Record"
        let a: [NSAttributedString.Key: Any] = [
            .font: theme.font(theme.type.label),
            .foregroundColor: theme.label,
        ]
        let size = s.size(withAttributes: a)
        s.draw(at: CGPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: a)
    }

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .button }
    public override func accessibilityLabel() -> String? { "Record Key for \(title)" }
    public override func accessibilityPerformPress() -> Bool {
        start()
        return true
    }
}
