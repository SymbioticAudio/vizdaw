import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// A command's first key as a menu shortcut (plan 4f Ruling 6): menus
/// show the bindings, so a rebound key shows at once.
@MainActor
enum KeyMenu {
    static func shortcut(_ id: String) -> KeyboardShortcut? {
        guard let r = keyBindings.row(id), let c = r.menuKey?.first else { return nil }
        var m: EventModifiers = []
        if r.menuModifiers.shift { m.insert(.shift) }
        if r.menuModifiers.control { m.insert(.control) }
        if r.menuModifiers.option { m.insert(.option) }
        if r.menuModifiers.command { m.insert(.command) }
        return KeyboardShortcut(KeyEquivalent(c), modifiers: m)
    }
}

/// Keys by the bindings (plan 4f Ruling 5): every key press in VizDAW
/// passes here before AppKit's menus see it. A text field, or a plugin
/// window's view, keeps its plain keys; an input method composing keeps
/// every key. In a window showing a document, a bound key runs its
/// command (or a sequence's keyboard slot) through the same handlers as
/// the control channel. A plain key not used up goes to its window alone,
/// never to the main menu (whose single-key items match by character, not
/// by key code); a ⌘/⌃ key not used up goes on to the menus.
@MainActor
enum KeyMonitor {
    private static var monitor: Any?
    private static var upMonitor: Any?
    private static var observers: [any NSObjectProtocol] = []
    /// The held ⏪ or ⏩ key (plan 7d Ruling 4): its key code and the
    /// shuttle its steps went to, which its key-up lands.
    private static var held: (keyCode: UInt16, shuttle: TransportShuttle)?

    static func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            // Key events arrive on the main thread.
            nonisolated(unsafe) let event = e
            let used = MainActor.assumeIsolated { consume(event) }
            return used ? nil : e
        }
        upMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyUp) { e in
            nonisolated(unsafe) let event = e
            MainActor.assumeIsolated { keyUp(event.keyCode) }
            return e
        }
        // The window or the app losing the keyboard ends a held key too:
        // its key-up would go elsewhere.
        let center = NotificationCenter.default
        for name in [NSWindow.didResignKeyNotification, NSApplication.didResignActiveNotification] {
            observers.append(
                center.addObserver(forName: name, object: nil, queue: .main) { _ in
                    MainActor.assumeIsolated { endHeld() }
                })
        }
    }

    /// A key-up: the held key's lands its hold once.
    static func keyUp(_ keyCode: UInt16) {
        guard held?.keyCode == keyCode else { return }
        endHeld()
    }

    /// Lands the held key's hold, if one is held.
    static func endHeld() {
        let h = held
        held = nil
        h?.shuttle.keyUp()
    }

    /// A key-down or repeat of ⏪ or ⏩ in a document's window: one key
    /// step on its shuttle (gated as the command is); whether the key was
    /// used up. An unavailable step passes the key on, as the command's
    /// would.
    static func keyStep(_ id: String, _ e: NSEvent, _ m: DocumentModel) -> Bool {
        if let h = held, h.shuttle !== m.shuttle || h.keyCode != e.keyCode { endHeld() }
        switch m.shuttle.keyStep(forward: id == "transport.fast_forward") {
        case .done:
            held = (e.keyCode, m.shuttle)
            return true
        case .unavailable: return false
        case .refused, .busy: return true
        }
    }

    static func modifiers(_ f: NSEvent.ModifierFlags) -> ModifiersRow {
        ModifiersRow(
            shift: f.contains(.shift), control: f.contains(.control), option: f.contains(.option),
            command: f.contains(.command))
    }

    /// What a key press in `w` does: the route, decided as for the
    /// channel's keys (`menus`: AppKit's menus see the key next).
    static func route(_ e: NSEvent, in w: NSWindow, menus: Bool) -> (KeyRoute, (id: UUID, model: DocumentModel)?) {
        // The Keys settings' recorder takes every key it is shown.
        if w.firstResponder is KeyCapturing { return (.pass, nil) }
        let tag = ControlWindows.tagOf(w)
        let typing = KeyRouter.typing(textField: w.firstResponder is NSText, window: tag.kind)
        let doc: (id: UUID, model: DocumentModel)? = tag.doc.flatMap { d in
            appStore.model(d).map { (d, $0) }
        }
        let m = modifiers(e.modifierFlags)
        let r = KeyRouter.route(
            keyCode: e.keyCode, modifiers: m, typing: typing, stepEntry: doc?.model.stepEntry.isOn ?? false,
            menus: menus, keys: keyBindings, sequences: doc?.model.snapshot.sequences ?? [])
        return (r, doc)
    }

    /// Runs the route's command (a window without a document runs only
    /// commands that need none); whether the key was used up.
    static func perform(_ r: KeyRoute, _ doc: (id: UUID, model: DocumentModel)?) -> Bool {
        let after: KeyAfter
        switch r {
        case .typing, .pass: return false
        case .slot(let seq):
            guard let doc else { return false }
            switch doc.model.pressSlot(seq) {
            case .done: return true
            case .unavailable: return false
            case .refused(let why), .busy(let why):
                doc.model.editor.status = why
                return true
            }
        case .command(let id):
            // Reload External Edit as its menu item (final review M9).
            after = KeyDispatch.command(id, doc: doc, store: appStore, host: AppControlHost.shared)
        }
        switch after {
        case .handled: return true
        case .pass: return false
        case .opened(let d):
            ControlWindows.openDocument(d)
            return true
        }
    }

    /// Whether VizDAW used the key press up (AppKit never sees it).
    static func consume(_ e: NSEvent) -> Bool {
        guard let w = e.window ?? NSApp.keyWindow else { return false }
        // An input method composing: the key is its own (Return confirms
        // the text, not a sheet's default button; fix round 1 I3).
        if let input = w.firstResponder as? NSTextInputClient, input.hasMarkedText() {
            w.sendEvent(e)
            return true
        }
        // Esc cancels a waiting MIDI learn (plan 5f-1 Task 11), wherever
        // it is pressed, except into the Keys settings' recorder, a modal
        // dialog or a text field, whose own Esc it is (final review T11 m7).
        if e.keyCode == 53, e.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty,
            !(w.firstResponder is KeyCapturing), NSApp.modalWindow == nil,
            !(w.firstResponder is NSTextView),
            let learning = appStore.documents.values.first(where: \.isLearning)
        {
            learning.learn.cancel()
            return true
        }
        let (r, doc) = route(e, in: w, menus: true)
        // A held key: toggles, transport keys and slots run once per press
        // (final review item 8).
        if e.isARepeat, !KeyDispatch.runsOnRepeat(r) { return true }
        // A held ⏪ or ⏩ key is one hold, landed on key-up (plan 7d
        // Ruling 4); a surface's button and the channel's keys step as a
        // click does.
        if case .command(let id) = r, KeyDispatch.held.contains(id), let m = doc?.model {
            if keyStep(id, e, m) { return true }
        } else if perform(r, doc) {
            return true
        }
        switch KeyDispatch.passed(modifiers: modifiers(e.modifierFlags)) {
        case .menus:
            // The Keys settings' recorder takes ⌘ and ⌃ keys before the
            // menus do.
            if w.firstResponder is KeyCapturing, w.performKeyEquivalent(with: e) { return true }
            return false
        case .window:
            // Past the main menu (its single-key items match by
            // character, not by key code), but the window's own key
            // equivalents first: Return and Escape still press a sheet's
            // default and cancel buttons (controller ruling F1), and the
            // recorder takes its key.
            if w.performKeyEquivalent(with: e) { return true }
            w.sendEvent(e)
            return true
        }
    }
}
