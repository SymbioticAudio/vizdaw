import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let bar: Int64 = 61_440
private let beat: Int64 = 15_360

/// The transport fields, their keys and the Keys settings' recorder
/// (plan 4f Task 10).
@MainActor
@Suite struct TransportFieldsTests {
    func model() throws -> (FakeSession, DocumentModel) {
        let f = demoFake()
        return (f, try DocumentModel(session: f))
    }

    @Test func fieldsShowAndTakeBarBeatUnit() throws {
        let (f, m) = try model()
        f.state.tick = bar + beat
        m.poll()
        #expect(m.fieldText(.position) == "2.2.0")
        #expect(m.fieldText(.editIn).isEmpty && m.fieldText(.punchOut) == "5.1.0")
        #expect(m.commitField(.position, "3"))
        #expect(f.state.tick == 2 * bar, "a bar alone is its downbeat")
        #expect(m.commitField(.position, "4:2:480"), "colons separate like dots")
        #expect(f.state.tick == 3 * bar + beat + beat / 2)
        #expect(m.commitField(.position, ".3"), "only the beat: the bar and unit stay")
        #expect(f.state.tick == 3 * bar + 2 * beat + beat / 2)
        #expect(!m.commitField(.position, "x.y"))
        #expect(m.editor.status == "“x.y” is not a position (bar.beat.unit).")
    }

    @Test func editInAndOutSetTheSelectionsRange() throws {
        let (_, m) = try model()
        #expect(!m.commitField(.editOut, "3"))
        #expect(m.editor.status == "Set Edit In first.")
        #expect(m.commitField(.editIn, "2"))
        #expect(m.editor.selection.timeRange == TickRange(start: bar, end: 2 * bar), "a bar long")
        #expect(m.commitField(.editOut, "4"))
        #expect(m.editor.selection.timeRange == TickRange(start: bar, end: 3 * bar))
        #expect(!m.commitField(.editOut, "1"))
        #expect(m.editor.status == "Edit Out must be after Edit In.")
        #expect(m.commitField(.editIn, "5"), "past the end: the length moves along")
        #expect(m.editor.selection.timeRange == TickRange(start: 4 * bar, end: 6 * bar))
        #expect(m.fieldText(.editIn) == "5.1.0" && m.fieldText(.editOut) == "7.1.0")
    }

    @Test func punchFieldsKeepTheirOrder() throws {
        let (_, m) = try model()
        #expect(m.commitField(.punchIn, "2"))
        #expect(m.editor.layout.transport.punchIn == bar && m.editor.layout.transport.punchOut == 4 * bar)
        #expect(m.commitField(.punchIn, "9"), "past Punch Out: it moves a bar after")
        #expect(m.editor.layout.transport.punchOut == 9 * bar)
        #expect(!m.commitField(.punchOut, "8"))
        #expect(m.editor.status == "Punch Out must be after Punch In.")
    }

    @Test func theFieldsViewCommitsRevertsAndNamesItsFields() throws {
        let (f, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        #expect(TransportFieldsView.find(in: w.contentView?.superview) === v)
        let pos = try #require(v.fields[.position])
        #expect(pos.accessibilityLabel() == "Position" && pos.stringValue == "1.1.0")
        w.makeFirstResponder(pos)
        #expect(pos.currentEditor() == nil, "not as the window's first text field")
        #expect(v.focus(.position) && w.firstResponder === pos.currentEditor())
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        #expect(m.editor.textEditors == [TransportFieldsView.textOwner])
        pos.stringValue = "2"
        v.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: pos,
                userInfo: ["NSTextMovement": NSTextMovement.return.rawValue]))
        #expect(f.state.tick == bar && !m.editor.textEditing)
        v.focus(.position)
        v.giveKeyboardBack(.position)
        #expect(w.firstResponder === w, "Return gives the keyboard back to the keys")
        // Escape reverts: nothing moves.
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        pos.stringValue = "9"
        #expect(v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        #expect(f.state.tick == bar && !m.editor.textEditing)
        m.poll()
        v.refresh()
        #expect(pos.stringValue == "2.1.0")
        #expect(!v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))))
    }

    @Test func aFocusedFieldKeepsItsTextAndPositionFollowsAfterAnEdit() async throws {
        let (f, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        let pos = try #require(v.fields[.position])
        #expect(v.focus(.position))
        f.state.tick = 2 * bar
        m.poll()
        v.refresh()
        #expect(pos.stringValue == "1.1.0", "focused but untyped: not overwritten")
        // Revert, then playing moves Position again (the tracking survives).
        #expect(v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        for t in [3 * bar, 4 * bar] {
            f.state.tick = t
            m.poll()
            try await Task.sleep(for: .milliseconds(50))
        }
        #expect(pos.stringValue == "5.1.0")
        w.makeFirstResponder(nil)
        // Commit, then the same.
        #expect(v.focus(.position))
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        pos.stringValue = "2"
        v.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: pos,
                userInfo: ["NSTextMovement": NSTextMovement.return.rawValue]))
        v.giveKeyboardBack(.position)
        for t in [6 * bar, 7 * bar] {
            f.state.tick = t
            m.poll()
            try await Task.sleep(for: .milliseconds(100))
        }
        #expect(pos.stringValue == "8.1.0")
    }

    @Test func fieldKeysFocusTheirField() throws {
        let host = FakeControlHost()
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        for (cmd, field) in [("transport.go_position", "position"), ("transport.go_edit_in", "editIn"),
                             ("transport.go_punch", "punchIn")] {
            guard case .done = ControlCommands.run(cmd, args: [], doc: (id, m), store: s, host: host) else {
                Issue.record("\(cmd) not done")
                continue
            }
            #expect(host.calls.last == "focus \(field)")
        }
        host.fieldsShown = false
        guard case .failed(let e) = ControlCommands.run("transport.go_punch", args: [], doc: (id, m), store: s, host: host)
        else {
            Issue.record("no fields, no focus")
            return
        }
        #expect(e.code == .unavailable)
    }

    @Test func theRecorderTakesOneKeyAndEscapeCancels() throws {
        let r = KeyRecorderView()
        r.title = "Stop"
        var got: [String] = []
        r.onKey = { got.append($0) }
        #expect(r.accessibilityLabel() == "Record Key for Stop" && r.accessibilityRole() == .button)
        func key(_ code: UInt16, _ flags: NSEvent.ModifierFlags) throws -> NSEvent {
            try #require(
                NSEvent.keyEvent(
                    with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: 0, context: nil,
                    characters: "k", charactersIgnoringModifiers: "k", isARepeat: false, keyCode: code))
        }
        #expect(!r.performKeyEquivalent(with: try key(40, [.command])), "not recording: the menus' key")
        #expect(r.accessibilityPerformPress() && r.recording)
        #expect(r.performKeyEquivalent(with: try key(40, [.command, .shift])))
        #expect(got == ["⇧⌘K"] && !r.recording)
        r.start()
        r.keyDown(with: try key(0xFFFF, []))
        #expect(r.recording && got.count == 1, "a key VizDAW cannot bind: still recording")
        r.keyDown(with: try key(53, []))
        #expect(!r.recording && got.count == 1, "Escape cancels")
        #expect((r as AnyObject) is KeyCapturing)
    }

    @Test func aRecordedKeyThatIsTakenWaitsForReplace() throws {
        let k = KeyBindings(defaults: MemoryDefaults())
        k.record("transport.stop", key: "X")
        #expect(k.row("transport.stop")?.keys == ["X"] && k.pending == nil)
        k.record("transport.stop", key: "Space")
        #expect(k.pending == .init(id: "transport.stop", key: "Space"))
        #expect(k.message == "Space is the key of “Play / Stop”. Replace it?")
        k.confirmReplace()
        #expect(k.row("transport.stop")?.keys == ["Space"] && k.row("transport.play_stop")?.keys == [])
        k.record("transport.play_stop", key: "Space")
        k.cancelReplace()
        #expect(k.pending == nil && k.message == nil && k.row("transport.play_stop")?.keys == [])
    }
}
