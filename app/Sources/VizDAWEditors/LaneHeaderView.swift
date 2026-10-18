import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// Lane headers beside the arrangement (Plan Ruling 24): name (double-click
/// renames), mute/solo/arm, destination menu, and an "Add Lane" row.
@MainActor
public final class LaneHeaderView: NSView, NSTextFieldDelegate {
    public let document: DocumentModel
    var editor: EditorModel { document.editor }
    public var verticalOffset: CGFloat = 0 { didSet { needsDisplay = true } }
    /// The lane being renamed, fixed when editing began (the shown
    /// sequence may change before it ends).
    private(set) var renaming: (seq: UInt64, lane: UInt64, name: String, field: NSTextField)?
    /// VoiceOver children, kept so their identity is stable between queries.
    private var a11yElements: [CanvasElement] = []

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    var laneHeight: CGFloat { CGFloat(editor.layout.laneHeight) }

    private func observe() {
        withObservationTracking {
            _ = document.lanes
            _ = editor.layout
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    /// Top of row `i` in this view (below a ruler-high spacer).
    func rowTop(_ i: Int) -> CGFloat {
        EditorCanvas.rulerHeight + CGFloat(i) * laneHeight - verticalOffset
    }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let small = NSFont.systemFont(ofSize: 11)
        for (i, lane) in document.lanes.enumerated() {
            let top = rowTop(i)
            guard top + laneHeight >= dirty.minY, top <= dirty.maxY else { continue }
            for (control, r) in LaneHeaderLayout.controls(rowTop: top) {
                switch control {
                case .name:
                    NSString(string: lane.name).draw(
                        in: r, withAttributes: [.font: NSFont.systemFont(ofSize: 12, weight: .medium)])
                case .mute: toggle("M", on: lane.mute, color: .systemYellow, in: r)
                case .solo: toggle("S", on: lane.solo, color: .systemGreen, in: r)
                case .arm: toggle("R", on: lane.arm, color: .systemRed, in: r)
                case .destination:
                    NSString(string: "▾ " + (lane.destination ?? "No Destination")).draw(
                        in: r, withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
                }
            }
            NSColor.separatorColor.setFill()
            NSRect(x: 0, y: top + laneHeight - 0.5, width: bounds.width, height: 1).fill()
        }
        let add = rowTop(document.lanes.count)
        NSString(string: "+ Add Lane").draw(
            at: CGPoint(x: 8, y: add + 6),
            withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: EditorCanvas.rulerHeight).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    private func toggle(_ letter: String, on: Bool, color: NSColor, in r: CGRect) {
        let box = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
        (on ? color : NSColor.quaternaryLabelColor).setFill()
        box.fill()
        let s = NSString(string: letter)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 10, weight: .bold),
            .foregroundColor: on ? NSColor.black : NSColor.secondaryLabelColor,
        ]
        let size = s.size(withAttributes: attrs)
        s.draw(at: CGPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: attrs)
    }

    private func row(at y: CGFloat) -> Int? {
        guard laneHeight > 0, laneHeight.isFinite else { return nil }
        let i = Int(((y - EditorCanvas.rulerHeight + verticalOffset) / laneHeight).rounded(.down))
        return i >= 0 ? i : nil
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y) else { return }
        let lanes = document.lanes
        guard i < lanes.count else {
            if i == lanes.count {
                editor.edit(.lane(edit: .add(seq: seq, name: "Lane \(lanes.count + 1)", index: nil)))
            }
            return
        }
        let lane = lanes[i]
        switch LaneHeaderLayout.control(at: p, rowTop: rowTop(i)) {
        case .mute: flags(seq, lane.id, mute: !lane.mute)
        case .solo: flags(seq, lane.id, solo: !lane.solo)
        case .arm: flags(seq, lane.id, arm: !lane.arm)
        case .destination: destinationMenu(seq: seq, lane: lane, at: p)
        case .name where e.clickCount >= 2: beginRename(lane, seq: seq, row: i)
        case .name, nil: break
        }
    }

    /// Right-click or Control-click on a lane: its menu.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y), document.lanes.indices.contains(i) else {
            return nil
        }
        return laneMenu(seq: seq, lane: document.lanes[i])
    }

    /// Drum Lane: marks the lane a drum lane (rows for its notes) or a
    /// normal lane again (Plan 4e Ruling 3).
    func laneMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let drum = MenuAction(title: "Drum Lane") { [weak self] in
            self?.editor.markDrumLane(seq: seq, lane: lane.id, on: !lane.isDrum)
        }
        menu.addItem(drum.item(checked: lane.isDrum))
        return menu
    }

    private func flags(_ seq: UInt64, _ lane: UInt64, mute: Bool? = nil, solo: Bool? = nil, arm: Bool? = nil) {
        editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane, mute: mute, solo: solo, arm: arm)))
    }

    private func destinationMenu(seq: UInt64, lane: LaneRow, at p: CGPoint) {
        destinationMenu(seq: seq, lane: lane).popUp(positioning: nil, at: p, in: self)
    }

    /// "No Destination" and every destination. When they cannot be read the
    /// menu says so (a disabled item) instead of looking empty; a failure
    /// other than Busy also shows in the status line (spec §5).
    func destinationMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let none = MenuAction(title: "No Destination") { [weak self] in
            self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: nil)))
        }
        menu.addItem(none.item(checked: lane.midiDest == nil))
        let dests: [DestRow]
        do {
            dests = try document.session.destinations()
        } catch {
            var busy = false
            if case .Busy? = error as? VizError { busy = true }
            if !busy { editor.report(error) }
            let item = NSMenuItem(
                title: busy ? "Destinations unavailable (busy)" : "Destinations unavailable",
                action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
            return menu
        }
        for d in dests {
            let a = MenuAction(title: d.name) { [weak self] in
                self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: d.id)))
            }
            menu.addItem(a.item(checked: lane.midiDest == d.id))
        }
        return menu
    }

    func beginRename(_ lane: LaneRow, seq: UInt64, row i: Int) {
        renaming?.field.removeFromSuperview()
        guard let r = LaneHeaderLayout.controls(rowTop: rowTop(i)).first(where: { $0.0 == .name })?.1 else { return }
        let field = NSTextField(string: lane.name)
        field.frame = r.insetBy(dx: -2, dy: -2)
        field.delegate = self
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (seq, lane.id, lane.name, field)
        editor.beginTextEditing("laneName")
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("laneName")
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        r.field.removeFromSuperview()
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        guard movement != .cancel else { return }
        if !name.isEmpty, name != r.name {
            editor.edit(.lane(edit: .rename(seq: r.seq, lane: r.lane, name: name)))
        }
    }

    /// Escape cancels a rename: the name stays and no edit is sent (the
    /// gap parked from plan 4e, closed like the drum row header's).
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        cancelRename()
        return true
    }

    /// Ends a rename without renaming.
    func cancelRename() {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("laneName")
        r.field.abortEditing()
        r.field.removeFromSuperview()
        window?.makeFirstResponder(nil)
    }

    /// A header torn down mid-rename never gets its end-editing callback;
    /// give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil, renaming != nil {
            renaming = nil
            editor.endTextEditing("laneName")
        }
    }

    public override func scrollWheel(with e: NSEvent) {
        nextResponder?.scrollWheel(with: e)
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Lane Headers" }

    public override func accessibilityChildren() -> [Any]? {
        guard let seq = document.selectedSeq else {
            a11yElements = []
            return []
        }
        let items = document.lanes.enumerated().map { i, lane in
            let r = NSRect(x: 0, y: rowTop(i), width: bounds.width, height: laneHeight)
            let item = CanvasAccessibilityItem(
                label: ArrangementA11y.header(lane), frame: r, selected: false,
                press: { [weak self] in self?.flags(seq, lane.id, mute: !lane.mute) })
            return (item: item, frame: r)
        }
        // Reused like EditorCanvas's, so their identity is stable between queries.
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}

/// A menu item that runs a closure.
@MainActor
final class MenuAction: NSObject {
    let title: String
    let run: @MainActor () -> Void
    private static var alive: [MenuAction] = []

    init(title: String, run: @escaping @MainActor () -> Void) {
        self.title = title
        self.run = run
    }

    func item(checked: Bool) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: #selector(fire), keyEquivalent: "")
        i.target = self
        i.state = checked ? .on : .off
        Self.alive.append(self)
        if Self.alive.count > 512 { Self.alive.removeFirst(256) }
        return i
    }

    @objc func fire() { run() }
}
