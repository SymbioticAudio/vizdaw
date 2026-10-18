import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The transport bar's fields (parent spec §9, plan 4f Ruling 16):
/// Position, Edit In and Out, Punch In and Out, each a text field showing
/// bar.beat.unit. A key (P, E, U) focuses one; Enter commits the typed
/// position, Escape reverts. Each field is an accessibility element named
/// by its title, so `vizctl find Position` reaches it.
@MainActor
public final class TransportFieldsView: NSView, NSTextFieldDelegate {
    /// The text-editing owner while a field has the keyboard.
    public static let textOwner = "transport"
    public let document: DocumentModel
    public private(set) var fields: [TransportField: NSTextField] = [:]
    /// The field being typed in.
    private(set) var editing: TransportField?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        for f in TransportField.allCases {
            let label = NSTextField(labelWithString: f.title)
            label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            let field = TransportTextField(string: "")
            field.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
            field.alignment = .right
            field.delegate = self
            field.title = f.title
            field.identifier = NSUserInterfaceItemIdentifier("transport.\(f.rawValue)")
            field.widthAnchor.constraint(equalToConstant: 86).isActive = true
            fields[f] = field
            stack.addArrangedSubview(label)
            stack.addArrangedSubview(field)
            if f == .position || f == .editOut { stack.setCustomSpacing(16, after: field) }
        }
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    private func observe() {
        withObservationTracking {
            refresh()
        } onChange: { [weak self] in
            DispatchQueue.main.async { self?.observe() }
        }
    }

    /// Every field not being typed in shows its position.
    func refresh() {
        for (f, field) in fields where f != editing {
            field.stringValue = document.fieldText(f)
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

    public func controlTextDidBeginEditing(_ obj: Notification) {
        editing = field(of: obj)
        document.editor.beginTextEditing(Self.textOwner)
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let f = field(of: obj) ?? editing, let text = fields[f]?.stringValue else { return }
        editing = nil
        document.editor.endTextEditing(Self.textOwner)
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        if movement != .cancel, text != document.fieldText(f) { document.commitField(f, text) }
        (fields[f] as? TransportTextField)?.armed = false
        refresh()
        // Enter commits and gives the keyboard back to the keys (Tab moves
        // on to the next field). Next turn: the field selects its text
        // again after this notification.
        if movement == .return {
            DispatchQueue.main.async { [weak self] in self?.giveKeyboardBack(f) }
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
        if window == nil, editing != nil {
            editing = nil
            document.editor.endTextEditing(Self.textOwner)
        }
    }
}

/// A transport field takes the keyboard only when clicked or focused by
/// its key, never as the window's first text field when it opens, so the
/// transport and step keys work from the start (plan 4f Ruling 16).
final class TransportTextField: NSTextField {
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
public final class KeyRecorderView: NSView, KeyCapturing {
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
        (recording ? NSColor.controlAccentColor : NSColor.separatorColor).setStroke()
        let p = NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4)
        p.stroke()
        let s = recording ? "Press a key…" : "Record"
        let a: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: NSColor.labelColor,
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
