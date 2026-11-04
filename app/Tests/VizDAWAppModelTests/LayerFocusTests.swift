import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// A layer the window hides ends its own edit first (plan 7a Rulings 3 and
/// 9; Task 6 review M3, fold check F3): `LayerFocus.resign` is what
/// `ResignWhenHidden` runs when its layer hides.

/// `content` at the bottom half of a 900×600 window, an empty view above it.
@MainActor
private func window(_ content: NSView) -> (NSWindow, upper: NSView) {
    let w = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 900, height: 600), styleMask: [.titled], backing: .buffered,
        defer: true)
    w.isReleasedWhenClosed = false
    let root = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 600))
    let upper = NSView(frame: NSRect(x: 0, y: 300, width: 900, height: 300))
    content.frame = NSRect(x: 0, y: 0, width: 900, height: 300)
    root.addSubview(upper)
    root.addSubview(content)
    w.contentView = root
    content.layout()
    return (w, upper)
}

@MainActor
private func rect(_ v: NSView) -> NSRect { v.convert(v.bounds, to: nil) }

@MainActor
private func deleteKey(_ w: NSWindow) -> NSEvent {
    NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: w.windowNumber,
        context: nil, characters: "\u{7F}", charactersIgnoringModifiers: "\u{7F}", isARepeat: false,
        keyCode: 51)!
}

/// A view that takes the keyboard.
@MainActor
private final class FocusView: NSView {
    override var acceptsFirstResponder: Bool { true }
}

@MainActor
@Suite struct LayerFocusTests {
    /// Task 7 review M2: a responder the hidden layer collapsed to nothing
    /// (its frame no longer inside the layer) still loses the keyboard.
    @Test func aCollapsedResponderIsResignedToo() {
        let content = NSView()
        let (w, upper) = window(content)
        defer { w.close() }
        let collapsed = FocusView(frame: NSRect(x: 10, y: 100, width: 0, height: 0))
        upper.addSubview(collapsed)
        #expect(w.makeFirstResponder(collapsed))
        #expect(LayerFocus.resign(w, inside: rect(content)))
        #expect(w.firstResponder !== collapsed)
        // The control: a visible responder outside the layer keeps it.
        let shown = FocusView(frame: NSRect(x: 10, y: 100, width: 50, height: 20))
        upper.addSubview(shown)
        #expect(w.makeFirstResponder(shown))
        #expect(!LayerFocus.resign(w, inside: rect(content)))
        #expect(w.firstResponder === shown)
    }
    @Test func hidingTheBottomPaneEndsAListCellEdit() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        let v = ListEditorView(model: m.list(.pane))
        let (w, upper) = window(v)
        defer { w.close() }
        v.table.layoutSubtreeIfNeeded()
        v.beginEditing(row: 0, column: .position)
        #expect(m.textFieldHasKeyboard && m.list(.pane).editing != nil)
        // A layer elsewhere hiding leaves the edit alone.
        #expect(!LayerFocus.resign(w, inside: rect(upper)))
        #expect(m.textFieldHasKeyboard && m.list(.pane).editing != nil)
        // ⌃⌘↓: the bottom pane hides, its edit ends through the list's own end path.
        #expect(LayerFocus.resign(w, inside: rect(v)))
        #expect(!m.textFieldHasKeyboard && m.list(.pane).editing == nil)
        #expect(m.transportKeysEnabled, "Space plays again")
        #expect(f.edits.isEmpty, "nothing typed, nothing changed")
        #expect(!LayerFocus.resign(w, inside: rect(v)), "nothing left to end")
    }

    @Test func deleteAfterHidingThePianoRollDeletesNothing() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let model = PianoRollModel.pane(m)
        let v = PianoRollView(model: model)
        let (w, _) = window(v)
        defer { w.close() }
        model.select([(LaneKey(3, 31), 1)], adding: false)
        #expect(w.makeFirstResponder(v))
        #expect(LayerFocus.resign(w, inside: rect(v)))
        #expect(w.firstResponder !== v)
        w.firstResponder?.keyDown(with: deleteKey(w))
        #expect(f.edits.isEmpty, "the hidden roll's notes stay")
        // The control: while shown, Delete does delete.
        #expect(w.makeFirstResponder(v))
        w.firstResponder?.keyDown(with: deleteKey(w))
        #expect(f.edits.last == .event(edit: .delete(events: [1])))
    }
}
