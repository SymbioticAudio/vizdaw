import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
private func listView(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws
    -> (FakeSession, DocumentModel, ListEditorView)
{
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(target, kind: .list)
    let v = ListEditorView(model: m.list(.pane))
    v.frame = NSRect(x: 0, y: 0, width: 900, height: 300)
    v.layout()
    return (f, m, v)
}

@MainActor
private func cell(_ v: ListEditorView, row: Int, column: Int) -> String? {
    (v.tableView(v.table, viewFor: v.table.tableColumns[column], row: row) as? NSTextField)?.stringValue
}

/// The view in a window, so the table really edits its cells.
@MainActor
private func windowed(_ v: ListEditorView) -> NSWindow {
    let w = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 900, height: 300), styleMask: [.titled], backing: .buffered,
        defer: true)
    w.isReleasedWhenClosed = false
    w.contentView = v
    v.layout()
    v.table.layoutSubtreeIfNeeded()
    return w
}

private let d4Loud = NoteInRow(pitch: 62, velocity: 0xC924, channel: 0)

@MainActor
@Suite struct ListEditorViewTests {
    @Test func rowsAndColumnsShow() throws {
        let (_, _, v) = try listView()
        #expect(v.shown.map(\.id) == ["e1", "e2"])
        #expect(v.table.tableColumns.map(\.title) == ["Position", "Length", "Kind", "Destination", "Data 1", "Data 2", "Mute"])
        #expect(v.numberOfRows(in: v.table) == 2)
        #expect(cell(v, row: 1, column: 0) == "1.2.0")
        #expect(cell(v, row: 1, column: 4) == "E4")
    }

    @Test func editingACellTurnsTheTransportKeysOffUntilItEnds() throws {
        let (f, m, v) = try listView()
        let field = NSTextField(string: "1.1.0")
        v.began(row: "e1", column: .position, field: field)
        #expect(!m.transportKeysEnabled)
        #expect(m.list(.pane).editing == ListCell(row: "e1", column: .position))
        field.stringValue = "2"
        v.ended(movement: .return)
        #expect(m.transportKeysEnabled)
        #expect(f.edits.last == .event(edit: .move(events: [1], delta: 61_440, toLane: nil)))
        #expect(m.list(.pane).cursor == "e1", "the list follows the moved event")
        #expect(m.list(.pane).editing == nil)
    }

    @Test func escapeLeavesTheEventAlone() throws {
        let (f, m, v) = try listView()
        let field = NSTextField(string: "1.1.0")
        v.began(row: "e1", column: .position, field: field)
        field.stringValue = "9"
        v.ended(movement: .cancel, cancelled: true)
        #expect(f.edits.isEmpty)
        #expect(!m.editor.textEditing)
        #expect(m.list(.pane).editing == nil)
    }

    @Test func leavingTheWindowEndsTheEdit() throws {
        let (f, m, v) = try listView()
        v.began(row: "e1", column: .position, field: NSTextField(string: "1.1.0"))
        v.viewWillMove(toWindow: nil)
        #expect(!m.editor.textEditing && f.edits.isEmpty)
        #expect(m.list(.pane).editing == nil)
    }

    @Test func aPlayedNoteFillsTheEditedCell() throws {
        let (_, m, v) = try listView()
        let l = m.list(.pane)
        l.midiIn = true
        let field = NSTextField(string: "C4")
        v.began(row: "e1", column: .data1, field: field)
        l.receive([d4Loud])
        v.applyInput()
        #expect(field.stringValue == "D4")
        #expect(l.fieldInput == nil)
    }

    @Test func keysDeleteAndOpenReferencesAndFocusPicksTheList() throws {
        let (f, m, v) = try listView()
        v.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        #expect(m.list(.pane).cursor == "e1")
        #expect(!v.key(125, modifiers: .command), "⌘↓ on a note opens nothing")
        #expect(v.key(51, modifiers: []))
        #expect(f.edits.last == .event(edit: .delete(events: [1])))
        v.focusChanged(true)
        #expect(m.activeList == .pane && m.listForCommands() === m.list(.pane))
        #expect(!m.returnKeyEnabled && m.transportKeysEnabled)
        v.focusChanged(false)
        #expect(m.returnKeyEnabled)
    }

    @Test func theTableIsAnAccessibleTable() throws {
        let (_, _, v) = try listView()
        #expect(v.table.accessibilityLabel() == "Event List")
        #expect(v.tableView(v.table, rowViewForRow: 0)?.accessibilityLabel() == "Note C4, bar 1 beat 1, velocity 64")
        #expect(
            v.table.tableColumns.map(\.headerCell.stringValue)
                == ["Position", "Length", "Kind", "Destination", "Data 1", "Data 2", "Mute"],
            "column headers show and read the titles")
        let field = try #require(v.tableView(v.table, viewFor: v.table.tableColumns[4], row: 1) as? NSTextField)
        // A plain text field: its cell is the accessibility element and reads the text.
        #expect(field.cell?.isAccessibilityElement() == true, "no custom cell hides the text")
        #expect(field.accessibilityValue() as? String == "E4", "cells read their text")
    }

    // MARK: Controller rulings

    @Test func theKeyboardIsTakenWhenTheCellEditStartsNotOnTheFirstKey() throws {
        let (f, m, v) = try listView()
        let w = windowed(v)
        defer { w.close() }
        v.beginEditing(row: 0, column: .position)
        #expect(m.editor.textEditing, "registered as soon as the field editor is in")
        #expect(!m.transportKeysEnabled && !m.returnKeyEnabled)
        #expect(m.list(.pane).editing == ListCell(row: "e1", column: .position))
        w.makeFirstResponder(v.table)
        #expect(!m.editor.textEditing && m.list(.pane).editing == nil, "ending without typing gives the keys back")
        #expect(f.edits.isEmpty, "nothing typed, nothing changed")
    }

    @Test func aCellEditGivesTheKeyboardBackWhileItsWindowIsNotKey() throws {
        let (f, m, v) = try listView()
        let w = windowed(v)
        defer { w.close() }
        v.beginEditing(row: 0, column: .position)
        #expect(m.editor.textEditing)
        NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: w)
        #expect(!m.editor.textEditing && m.transportKeysEnabled, "the other window's keys work")
        #expect(m.list(.pane).editing == ListCell(row: "e1", column: .position), "the edit stays open")
        NotificationCenter.default.post(name: NSWindow.didBecomeKeyNotification, object: w)
        #expect(m.editor.textEditing && !m.transportKeysEnabled, "back in the cell, it has the keyboard again")
        let editor = try #require(w.firstResponder as? NSTextView)
        editor.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        #expect(!m.editor.textEditing && f.edits.isEmpty)
        NotificationCenter.default.post(name: NSWindow.didBecomeKeyNotification, object: w)
        #expect(!m.editor.textEditing, "no edit open, nothing taken")
    }

    @Test func escapeInARealCellGivesTheKeysBack() throws {
        let (f, m, v) = try listView()
        let w = windowed(v)
        defer { w.close() }
        v.beginEditing(row: 0, column: .position)
        let editor = try #require(w.firstResponder as? NSTextView)
        editor.string = "9"
        editor.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        #expect(!m.editor.textEditing && m.list(.pane).editing == nil)
        #expect(f.edits.isEmpty)
    }

    @Test func aWindowListAndThePaneListEditIndependently() throws {
        let (_, m, pane) = try listView()
        let t = try #require(m.editor.tearOff(.list, shown: 3))
        let win = ListEditorView(model: m.list(.window(t.id)))
        pane.began(row: "e1", column: .position, field: NSTextField(string: "1.1.0"))
        win.began(row: "e2", column: .position, field: NSTextField(string: "1.2.0"))
        win.ended(movement: .cancel, cancelled: true)
        #expect(m.editor.textEditing, "the pane still edits")
        pane.ended(movement: .cancel, cancelled: true)
        #expect(!m.editor.textEditing)
    }

    @Test func switchingThePaneAwayFromTheListDropsIt() throws {
        let (_, m, v) = try listView()
        let l = m.list(.pane)
        l.midiIn = true
        v.focusChanged(true)
        #expect(m.lists[.pane] === l && m.activeList == .pane)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        #expect(m.lists[.pane] === l, "still a list: kept")
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        #expect(m.lists[.pane] == nil && m.activeList == nil && !m.listensForNotes)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        #expect(m.list(.pane) !== l)
        m.editor.closeDetail()
        #expect(m.lists[.pane] == nil)
    }

    @Test func aTornListGoesWhenItsWindowCloseOrChangesKind() throws {
        let (_, m, _) = try listView()
        let a = try #require(m.editor.tearOff(.list, shown: 3))
        let b = try #require(m.editor.tearOff(.list, shown: 3))
        _ = m.list(.window(a.id))
        _ = m.list(.window(b.id))
        m.editor.setWindowKind(a.id, .pianoRoll)
        #expect(m.lists[.window(a.id)] == nil)
        m.editor.closeWindow(b.id)
        #expect(m.lists[.window(b.id)] == nil)
        // A window closed with its document stays in the layout; its close
        // calls dropList.
        let c = try #require(m.editor.tearOff(.list, shown: 3))
        _ = m.list(.window(c.id))
        m.dropList(.window(c.id))
        #expect(m.lists[.window(c.id)] == nil)
        #expect(m.lists[.pane] != nil, "the pane's list is untouched")
    }

    @Test func closingTheDocumentDropsItsLists() throws {
        let s = DocumentStore(makeEmpty: { rollFake() }, makeDemo: { rollFake() }, open: { _ in rollFake() })
        let id = try #require(s.newEmpty())
        let m = try #require(s.model(id))
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        m.list(.pane).midiIn = true
        #expect(m.listensForNotes)
        s.close(id)
        #expect(m.lists.isEmpty && !m.listensForNotes)
    }

    @Test func playedNotesGoToTheCommandTargetDocument() throws {
        let s = DocumentStore(makeEmpty: { rollFake() }, makeDemo: { rollFake() }, open: { _ in rollFake() })
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        #expect(s.noteTarget == nil)
        let aWindow = UUID()
        s.focusDocument(a, window: aWindow)
        #expect(s.noteTarget === s.model(a))
        s.focusTorn(TornFocus(doc: b, window: UUID()))
        #expect(s.noteTarget === s.model(b), "a key torn window wins, as for menu commands")
        s.blurDocument(a, window: aWindow)
        s.focusPanel(a)
        #expect(s.noteTarget === s.model(b))
        s.close(b)
        #expect(s.noteTarget === s.model(a), "the key panel's document")
        s.blurPanel(a)
        #expect(s.noteTarget == nil)
    }

    @Test func aDocumentWindowAndItsMixerHandFocusOverInEitherOrder() throws {
        let s = DocumentStore(makeEmpty: { rollFake() }, makeDemo: { rollFake() }, open: { _ in rollFake() })
        let a = try #require(s.newEmpty())
        let docWindow = UUID()
        let mixer = UUID()
        s.focusDocument(a, window: docWindow)
        // The mixer becomes key before the document window hears it resigned.
        s.focusDocument(a, window: mixer)
        s.blurDocument(a, window: docWindow)
        #expect(s.noteTarget === s.model(a), "the late blur of the other window clears nothing")
        // And back: the mixer resigns first, then the document window takes it.
        s.blurDocument(a, window: mixer)
        s.focusDocument(a, window: docWindow)
        #expect(s.noteTarget === s.model(a))
        s.blurDocument(a, window: docWindow)
        #expect(s.noteTarget == nil, "the window that has it clears it")
    }

    @Test func aReadOnlyCellNeverTakesTheKeyboard() throws {
        let (_, m, v) = try listView()
        let w = windowed(v)
        defer { w.close() }
        let kind = try #require(v.shownColumns.firstIndex(of: .kind))
        let field = try #require(v.table.view(atColumn: kind, row: 0, makeIfNecessary: true) as? NSTextField)
        #expect(!field.isEditable && !field.isSelectable)
        v.fieldTookKeyboard(field)
        #expect(!m.editor.textEditing && m.list(.pane).editing == nil)
    }

    @Test func movingStraightToAnotherCellCommitsAndReloadsWhenThatEditEnds() throws {
        let (f, m, v) = try listView()
        let a = NSTextField(string: "1.1.0")
        v.began(row: "e1", column: .position, field: a)
        a.stringValue = "3"
        v.began(row: "e2", column: .position, field: NSTextField(string: "1.2.0"))
        #expect(f.edits.last == .event(edit: .move(events: [1], delta: 2 * 61_440, toLane: nil)))
        #expect(v.table.selectedRowIndexes.isEmpty, "the table waits while the next cell edits")
        #expect(m.editor.textEditing && m.list(.pane).editing == ListCell(row: "e2", column: .position))
        v.ended(movement: .return)
        #expect(v.table.selectedRowIndexes == IndexSet(integer: 0), "reloaded when that edit ended: on the moved event")
        #expect(!m.editor.textEditing)
    }
}
