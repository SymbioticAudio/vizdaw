import AppKit
import Foundation
import SymbioticTokens
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

    /// The B4 run found the field labels and values faint in Classic
    /// Light: both take the theme's text colour, from the start and on a
    /// change (plan 7b Task B5).
    @Test func theFieldsLabelsAndValuesAreInTheThemesText() throws {
        let (_, m) = try model()
        let v = TransportFieldsView(document: m)
        for t in [v.theme, .classicLight, .studioDark] {
            v.theme = t
            for f in TransportField.allCases {
                #expect(v.labels[f]?.textColor == t.label, "\(t.id) \(f) label")
                #expect(v.fields[f]?.textColor == t.label, "\(t.id) \(f) value")
                #expect(v.fields[f]?.backgroundColor == t.canvas, "\(t.id) \(f) background")
            }
        }
    }

    /// Task 7 review I1: the transport bar puts the fields (with step
    /// entry's summary in front) on a second row when one row cannot hold
    /// them, so none is cut at the default 980-point window or at 700.
    @Test func theTransportBarTakesTwoRowsWhenTheFieldsDoNotFit() throws {
        let (_, m) = try model()
        let v = TransportFieldsView(document: m)
        let fields = v.fittingWidth
        #expect(fields > 5 * 86 && fields < 900, "five 86-point fields and their labels: \(fields)")
        // The bar's other controls in the demo: Play to the position (325),
        // the device to the right pane toggle (345).
        func rows(_ width: CGFloat) -> Int {
            TransportBarRows.count(width: width, leading: 325, fields: fields, trailing: 345, spacing: 10)
        }
        #expect(rows(1600) == 1, "a wide window keeps one row")
        #expect(rows(980) == 2, "the default window")
        #expect(rows(700) == 2, "the minimum window")
        #expect(rows(325 + 10 + fields + 10 + 345 + 16) == 1, "exactly enough, with the bar's padding")
        #expect(rows(325 + 10 + fields + 10 + 345 + 15) == 2)
        #expect(rows(.nan) == 2 && rows(-1) == 2, "a bad width is never one row")
        // The second row at the 700-point minimum (700 − 16): every field
        // shows, narrowed from 86 towards 72, its label whole.
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 684, height: 22), styleMask: [.titled], backing: .buffered,
            defer: true)
        w.isReleasedWhenClosed = false
        defer { w.close() }
        // As SwiftUI sizes its host: a width the fields cannot push.
        let box = NSView(frame: NSRect(x: 0, y: 0, width: 684, height: 22))
        w.contentView = box
        v.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(v)
        NSLayoutConstraint.activate([
            v.leadingAnchor.constraint(equalTo: box.leadingAnchor), v.topAnchor.constraint(equalTo: box.topAnchor),
            v.widthAnchor.constraint(equalToConstant: 684), v.heightAnchor.constraint(equalToConstant: 22),
        ])
        box.layoutSubtreeIfNeeded()
        for f in TransportField.allCases {
            let r = try #require(v.fields[f]).convert(try #require(v.fields[f]).bounds, to: v)
            #expect(r.maxX <= 684 && r.width >= 72, "\(f): \(r)")
        }
        #expect(v.fittingWidth == fields, "the row rule still sees the full width")
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

    /// Lets the main queue run what is already on it (the fields view
    /// re-observes there, ruling F4): no fixed sleep (final review M-6).
    func mainQueueHop() async {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            DispatchQueue.main.async { c.resume() }
        }
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
            await mainQueueHop()
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
            await mainQueueHop()
        }
        #expect(pos.stringValue == "8.1.0")
    }

    /// Item 11: a punch range past the tick limit is refused, not
    /// reported as set.
    @Test func aPunchRangePastTheLimitIsRefused() throws {
        // 255/1 bars are 15 667 200 ticks: bar 70 180 starts within the
        // limit, and a bar after it is past it.
        let f = demoFake()
        var info = fourFour(3)
        info.meter = [MeterEntryRow(tick: 0, num: 255, den: 1, sharps: 0, minor: false)]
        f.infoTable[3] = info
        let m = try DocumentModel(session: f)
        #expect(m.selectedSeq == 3)
        let before = m.editor.layout.transport
        #expect(!m.commitField(.punchIn, "70180"), "Punch Out would pass the limit")
        #expect(m.editor.layout.transport == before)
        #expect(m.editor.status == TransportField.punchLimit)
    }

    /// I-2: Tab moves the keyboard to the next field and Shift-Tab back
    /// (Edit Out and Punch Out have no key of their own).
    @Test func tabMovesBetweenTheFields() throws {
        let (_, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        let editOut = try #require(v.fields[.editOut])
        let punchIn = try #require(v.fields[.punchIn])
        #expect(v.focus(.editIn))
        (w.firstResponder as? NSTextView)?.insertTab(nil)
        #expect(editOut.currentEditor() != nil && w.firstResponder === editOut.currentEditor(), "Tab: Edit Out")
        (w.firstResponder as? NSTextView)?.insertBacktab(nil)
        #expect(v.fields[.editIn]?.currentEditor() != nil, "Shift-Tab: Edit In again")
        #expect(v.focus(.punchIn))
        (w.firstResponder as? NSTextView)?.insertTab(nil)
        #expect(v.fields[.punchOut]?.currentEditor() != nil, "Tab: Punch Out")
        #expect(punchIn.currentEditor() == nil)
    }

    func endEditing(_ v: TransportFieldsView, _ field: NSTextField, _ movement: NSTextMovement?) {
        let info: [AnyHashable: Any]? = movement.map { ["NSTextMovement": $0.rawValue] }
        v.controlTextDidEndEditing(
            Notification(name: NSControl.textDidEndEditingNotification, object: field, userInfo: info))
    }

    /// Plan 7d Ruling 7: only Return, Tab and Shift-Tab commit; a click
    /// elsewhere (movement `.other`) or no movement at all cancels.
    @Test func clickingAwayCancelsATypedField() throws {
        let (f, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        let pos = try #require(v.fields[.position])
        for movement in [NSTextMovement.other, nil] {
            #expect(v.focus(.position))
            v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
            pos.stringValue = "5"
            endEditing(v, pos, movement)
            #expect(f.locates.isEmpty, "\(String(describing: movement)): nothing moves")
            #expect(pos.stringValue == m.fieldText(.position))
            #expect(m.editor.textEditors.isEmpty)
            w.makeFirstResponder(nil)
        }
    }

    /// Pre-flight C1: a press that does not take the keyboard (a bar
    /// button, the readouts) still ends the edit, and so does the window
    /// resigning key; a bar button then acts (controller ruling I-4).
    @Test func aMouseDownOutsideTheFieldEndsTheEdit() throws {
        #expect(TransportFieldsView.endsOnMouseDown(inField: false))
        #expect(!TransportFieldsView.endsOnMouseDown(inField: true))
        let (f, m) = try model()
        let v = TransportFieldsView(document: m)
        let readouts = TransportReadoutsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 900, height: 80), styleMask: [.titled],
                         backing: .buffered, defer: true)
        let content = NSView(frame: CGRect(x: 0, y: 0, width: 900, height: 80))
        w.contentView = content
        v.frame = CGRect(x: 0, y: 0, width: 700, height: 30)
        readouts.frame = CGRect(x: 0, y: 40, width: 400, height: 40)
        content.addSubview(v)
        content.addSubview(readouts)
        content.layoutSubtreeIfNeeded()
        let pos = try #require(v.fields[.position])
        let over = CGPoint(x: 100, y: 60)
        #expect(readouts.frame.contains(over) && !readouts.acceptsFirstResponder)
        let press = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: over, modifierFlags: [], timestamp: 0,
                windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        let ends: [(String, () -> Void)] = [
            ("a press on the readouts", { v.handleMouseDown(press) }),
            ("the window resigning key", { v.resignKeyCancels() }),
        ]
        for (what, end) in ends {
            #expect(v.focus(.position))
            v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
            pos.stringValue = "5"
            #expect(!m.transportKeysEnabled)
            end()
            #expect(w.firstResponder !== pos.currentEditor() && pos.currentEditor() == nil, "\(what)")
            #expect(f.locates.isEmpty, "\(what): nothing moves")
            #expect(pos.stringValue == m.fieldText(.position), "\(what): the old text")
            #expect(m.transportKeysEnabled, "\(what): a bar button then acts")
        }
    }

    /// A window holding the fields view and nothing else, laid out.
    /// Not deferred: a real window number, so a press's `window` is it.
    func laidOut(_ m: DocumentModel) -> (TransportFieldsView, NSWindow) {
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: false)
        w.contentView = v
        v.layoutSubtreeIfNeeded()
        return (v, w)
    }

    /// Fix round 1 M5: a press inside the field being edited (placing the
    /// caret) passes through: the edit, its text and its watch stay.
    @Test func aPressInsideTheFieldKeepsTheEdit() throws {
        let (f, m) = try model()
        let (v, w) = laidOut(m)
        let pos = try #require(v.fields[.position])
        #expect(v.focus(.position))
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        pos.stringValue = "5"
        let inside = pos.convert(CGPoint(x: pos.bounds.midX, y: pos.bounds.midY), to: nil)
        let press = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: inside, modifierFlags: [], timestamp: 0,
                windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        v.handleMouseDown(press)
        #expect(pos.currentEditor() != nil && w.firstResponder === pos.currentEditor())
        #expect(v.editing == .position && v.isWatching && pos.stringValue == "5")
        #expect(f.locates.isEmpty && !m.transportKeysEnabled)
    }

    /// Fix round 1 M5 (pre-flight C1): the mouse-down monitor and the
    /// resign-key observer go when the edit ends: Return, Esc, a click
    /// elsewhere, the window losing key and teardown.
    @Test func theEditsWatchIsRemovedWhenItEnds() throws {
        let (_, m) = try model()
        let (v, w) = laidOut(m)
        let pos = try #require(v.fields[.position])
        func begin() {
            v.focus(.position)
            v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
            #expect(v.isWatching)
        }
        begin()
        pos.stringValue = "2"
        endEditing(v, pos, .return)
        #expect(!v.isWatching, "Return")
        w.makeFirstResponder(nil)
        begin()
        #expect(v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        #expect(!v.isWatching, "Esc")
        begin()
        w.makeFirstResponder(nil)
        #expect(!v.isWatching, "the field editor resigning")
        begin()
        v.resignKeyCancels()
        #expect(!v.isWatching, "the window losing key")
        begin()
        w.contentView = nil
        #expect(!v.isWatching && m.editor.textEditors.isEmpty, "teardown")
    }

    /// Ruling 7: Tab commits and moves on (README 101 types Edit In, then
    /// tabs to Edit Out, as one entry).
    @Test func tabCommitsAndMovesOn() throws {
        #expect(TransportFieldsView.commits(.return))
        #expect(TransportFieldsView.commits(.tab) && TransportFieldsView.commits(.backtab))
        #expect(!TransportFieldsView.commits(.other) && !TransportFieldsView.commits(.cancel))
        #expect(!TransportFieldsView.commits(nil))
        let (_, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        let editIn = try #require(v.fields[.editIn])
        #expect(v.focus(.editIn))
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: editIn))
        editIn.stringValue = "3"
        endEditing(v, editIn, .tab)
        #expect(m.editor.selection.timeRange?.start == 2 * bar)
        #expect((v.fields[.editOut] as? TransportTextField)?.armed == true, "the next field is armed")
    }

    /// Plan 7a n6: P, E and U focus the key mixer window's own fields.
    @Test func fieldKeysGoToTheKeyMixerWindow() {
        let d = UUID()
        let other = UUID()
        #expect(TransportFieldsTarget.choose(keyKind: .mixer, keyDoc: d, doc: d) == .mixerWindow)
        #expect(TransportFieldsTarget.choose(keyKind: .mixer, keyDoc: other, doc: d) == .document)
        #expect(TransportFieldsTarget.choose(keyKind: .document, keyDoc: d, doc: d) == .document)
        #expect(TransportFieldsTarget.choose(keyKind: nil, keyDoc: nil, doc: d) == .document)
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
