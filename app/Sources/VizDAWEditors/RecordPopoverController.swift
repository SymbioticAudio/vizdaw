import AppKit
import VizDAWAppModel

/// The transport bars' Record buttons, so a press can be told apart
/// (plan 7d Ruling 7's bar Record rule, fix round 1 I1): each bar's
/// inert marker behind its Record button registers itself here.
@MainActor
public enum RecordButtons {
    private static let views = NSHashTable<NSView>.weakObjects()

    public static func register(_ v: NSView) { views.add(v) }

    public static func unregister(_ v: NSView) { views.remove(v) }

    /// `event` is a left mouse-down inside a registered Record button.
    public static func contains(_ event: NSEvent?) -> Bool {
        guard let event, event.type == .leftMouseDown, let w = event.window else { return false }
        return views.allObjects.contains { v in
            v.window === w && v.bounds.contains(v.convert(event.locationInWindow, from: nil))
        }
    }
}

/// The record popover as an AppKit `NSPopover` (fix round 1 I1): its close
/// veto reads the event that would close it, so a press on Record with a
/// punch draft open leaves it open, with the draft and its keyboard, and
/// Record refuses. Any other close sets the model's `showsPopover` false,
/// which drops the drafts.
@MainActor
public final class RecordPopoverController: NSObject, NSPopoverDelegate {
    public let record: RecordModel
    public let host: RecordPopoverHost
    public let popover = NSPopover()
    /// The view it shows under (the chevron): a press on it is the chevron's
    /// own toggle, not an outside click.
    public weak var anchor: NSView?
    /// The event being dispatched (tests give their own: posting one into
    /// `NSApp`'s queue to make it current ends the test run).
    var currentEvent: () -> NSEvent? = { NSApp.currentEvent }

    public init(record: RecordModel, host: RecordPopoverHost, content: NSViewController) {
        self.record = record
        self.host = host
        super.init()
        popover.behavior = .transient
        popover.animates = false
        popover.contentViewController = content
        popover.delegate = self
    }

    /// Shows or closes it to match the model.
    public func sync(shown: Bool) {
        if shown, !popover.isShown, let a = anchor, a.window != nil {
            popover.show(relativeTo: a.bounds, of: a, preferredEdge: .minY)
        } else if !shown, popover.isShown {
            popover.close()
        }
    }

    /// The close veto for `event`, the event being dispatched.
    func shouldClose(for event: NSEvent?) -> Bool {
        // The chevron's own press: its action toggles the model.
        if let a = anchor, let e = event, e.type == .leftMouseDown, e.window === a.window,
            a.bounds.contains(a.convert(e.locationInWindow, from: nil))
        {
            return false
        }
        return RecordModel.closeAllowed(pressOnRecord: RecordButtons.contains(event), draftOpen: record.hasOpenPunchDraft)
    }

    public func popoverShouldClose(_ popover: NSPopover) -> Bool {
        let event = currentEvent()
        let ok = shouldClose(for: event)
        if !ok, RecordButtons.contains(event) {
            // The press makes the bar's window key: give the keyboard back to
            // the popover's field, so Return sets the punch (fix round 1 M2).
            let p = popover
            DispatchQueue.main.async { p.contentViewController?.viewIfLoaded?.window?.makeKey() }
        }
        return ok
    }

    public func popoverWillClose(_ notification: Notification) {
        if record.showsPopover(in: host) { record.showsPopover = false }
    }
}
