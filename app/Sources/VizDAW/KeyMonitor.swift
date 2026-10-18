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
/// passes here before AppKit's menus see it. A text field keeps its
/// plain keys (they go straight to its window, past the menus'
/// single-key shortcuts); in a window showing a document, a bound key
/// runs its command (or a sequence's keyboard slot) through the same
/// handlers as the control channel. A ⌘/⌃ key whose command is
/// unavailable or needs its dialog, and an unbound key, go on to the
/// menus and the views; a plain key whose command is unavailable goes to
/// its window alone (controller ruling F2), so the main menu cannot
/// match it to another command's plain-key item.
@MainActor
enum KeyMonitor {
    private static var monitor: Any?

    static func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            // Key events arrive on the main thread.
            nonisolated(unsafe) let event = e
            let used = MainActor.assumeIsolated { consume(event) }
            return used ? nil : e
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
        let typing = w.firstResponder is NSText
        let doc: (id: UUID, model: DocumentModel)? = ControlWindows.tagOf(w).doc.flatMap { d in
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
        let outcome: ControlOutcome
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
            outcome = ControlCommands.run(id, args: [], doc: doc, store: appStore, host: AppControlHost.shared)
        }
        switch KeyDispatch.after(outcome, doc: doc?.model) {
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
        let (r, doc) = route(e, in: w, menus: true)
        if perform(r, doc) { return true }
        switch KeyDispatch.passed(r, modifiers: modifiers(e.modifierFlags)) {
        case .menus:
            return false
        case .window:
            // Past the main menu (its single-key shortcuts must not take
            // a letter typed into a field, nor a passed plain key), but
            // the window's own key equivalents first: Return and Escape
            // still press a sheet's default and cancel buttons
            // (controller ruling F1).
            if w.performKeyEquivalent(with: e) { return true }
            w.sendEvent(e)
            return true
        }
    }
}
