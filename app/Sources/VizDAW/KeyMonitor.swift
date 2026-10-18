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
        // An input method composing: the key is its own (Return confirms
        // the text, not a sheet's default button; fix round 1 I3).
        if let input = w.firstResponder as? NSTextInputClient, input.hasMarkedText() {
            w.sendEvent(e)
            return true
        }
        let (r, doc) = route(e, in: w, menus: true)
        // A held key: toggles, transport keys and slots run once per press
        // (final review item 8).
        if e.isARepeat, !KeyDispatch.runsOnRepeat(r) { return true }
        if perform(r, doc) { return true }
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
