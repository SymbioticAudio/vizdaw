import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The list's table: hands keys and focus changes to its view first.
public final class ListTable: NSTableView {
    weak var owner: ListEditorView?

    public override func keyDown(with e: NSEvent) {
        if owner?.key(e.keyCode, modifiers: e.modifierFlags) != true { super.keyDown(with: e) }
    }

    public override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { owner?.focusChanged(true) }
        return ok
    }

    public override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { owner?.focusChanged(false) }
        return ok
    }
}

/// A cell's text field: tells the list the moment it takes the keyboard
/// (the field editor goes in), before any key is typed, so a first Space
/// types a space and Return ends the edit.
final class ListCellField: NSTextField {
    weak var list: ListEditorView?

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { list?.fieldTookKeyboard(self) }
        return ok
    }
}

/// The list editor (spec §4c): one row per event (and tempo, meter and
/// marker entry), keyboard-first, edited in place. An `NSTableView` with
/// column headers and plain text-field cells, so VoiceOver reads it as a
/// table (Plan 4c Ruling 20).
@MainActor
public final class ListEditorView: NSView, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate {
    /// The text-editing owner while a cell is edited (Plan 4c Ruling 21);
    /// each slot's list registers its own, so two lists editing at once
    /// (the pane and a torn window) don't end each other's.
    public static let textOwner = "list"
    public let model: ListEditorModel
    let scroll = NSScrollView()
    let table = ListTable()
    private(set) var shown: [ListRow] = []
    private(set) var shownColumns: [ListColumn] = []
    private var context: ListContext
    /// The cell being edited: row, column, field and the text it started with.
    private(set) var editingCell: (row: String, column: ListColumn, field: NSTextField, original: String)?
    /// Selection changes made here, not by the user.
    private var syncing = false
    private var keyObservers: [NSObjectProtocol] = []

    private var textOwner: String { "\(Self.textOwner).\(model.slot)" }

    public init(model: ListEditorModel) {
        self.model = model
        context = model.context
        super.init(frame: .zero)
        table.owner = self
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = true
        table.usesAlternatingRowBackgroundColors = true
        table.rowHeight = 18
        table.style = .plain
        table.target = self
        table.doubleAction = #selector(doubleClicked)
        table.setAccessibilityLabel("Event List")
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        addSubview(scroll)
        observeRows()
        observeCursor()
        reload()
    }

    public required init?(coder: NSCoder) { nil }

    public override func layout() {
        super.layout()
        scroll.frame = bounds
    }

    // MARK: Observation

    private func observeRows() {
        withObservationTracking {
            _ = model.rows
            _ = model.columns
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.observeRows()
                self?.reload()
            }
        }
    }

    private func observeCursor() {
        withObservationTracking {
            _ = model.cursor
            _ = model.fieldInput
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.observeCursor()
                self?.applyInput()
                self?.syncCursor()
            }
        }
    }

    // MARK: Rows

    /// Re-reads rows and columns; waits while a cell is edited.
    func reload() {
        guard editingCell == nil else { return }
        let cols = model.columns
        if cols != shownColumns { rebuildColumns(cols) }
        shown = model.rows
        context = model.context
        syncing = true
        table.reloadData()
        syncing = false
        syncCursor()
    }

    private func rebuildColumns(_ cols: [ListColumn]) {
        for c in table.tableColumns { table.removeTableColumn(c) }
        for c in cols {
            let tc = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(c.rawValue))
            tc.title = c.title
            tc.width = Self.width(c)
            table.addTableColumn(tc)
        }
        shownColumns = cols
    }

    static func width(_ c: ListColumn) -> CGFloat {
        switch c {
        case .position, .end: 90
        case .duration: 70
        case .kind: 110
        case .lane: 90
        case .dest: 110
        case .data1: 90
        case .data2: 140
        case .release: 60
        case .smpte: 100
        case .mute: 44
        }
    }

    /// The table's selection and scroll follow the model's cursor.
    func syncCursor() {
        guard editingCell == nil, let c = model.cursor, let i = shown.firstIndex(where: { $0.id == c }) else {
            return
        }
        if !table.selectedRowIndexes.contains(i) {
            syncing = true
            table.selectRowIndexes(IndexSet(integer: i), byExtendingSelection: false)
            syncing = false
        }
        table.scrollRowToVisible(i)
    }

    /// A played note's text goes into the edited cell (Plan 4c Ruling 12).
    func applyInput() {
        guard let text = model.fieldInput else { return }
        model.fieldInput = nil
        guard let e = editingCell else { return }
        if let editor = e.field.currentEditor() {
            editor.string = text
            editor.selectAll(nil)
        } else {
            e.field.stringValue = text
        }
    }

    // MARK: Table

    public func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn, let column = ListColumn(rawValue: tableColumn.identifier.rawValue),
            shown.indices.contains(row)
        else { return nil }
        let r = shown[row]
        let field =
            (tableView.makeView(withIdentifier: tableColumn.identifier, owner: self) as? NSTextField)
            ?? makeField(tableColumn.identifier)
        field.stringValue = ListText.cell(r, column, context)
        field.isEditable = ListText.editable(r, column)
        // A read-only cell never takes the keyboard (nor turns the
        // transport keys off).
        field.isSelectable = field.isEditable
        field.textColor = field.isEditable ? .labelColor : .secondaryLabelColor
        return field
    }

    private func makeField(_ id: NSUserInterfaceItemIdentifier) -> NSTextField {
        let f = ListCellField()
        f.list = self
        f.identifier = id
        f.isBordered = false
        f.drawsBackground = false
        f.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        f.lineBreakMode = .byTruncatingTail
        f.delegate = self
        return f
    }

    public func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let v = NSTableRowView()
        if shown.indices.contains(row) { v.setAccessibilityLabel(ListText.summary(shown[row], context)) }
        return v
    }

    public func tableViewSelectionDidChange(_ notification: Notification) {
        guard !syncing else { return }
        let ids = table.selectedRowIndexes.compactMap { shown.indices.contains($0) ? shown[$0].id : nil }
        let r = table.selectedRow
        model.cursor = shown.indices.contains(r) ? shown[r].id : nil
        model.select(ids)
    }

    // MARK: Editing

    /// Starts editing a cell (Return, Tab, double-click).
    func beginEditing(row index: Int, column: ListColumn) {
        guard shown.indices.contains(index), let c = shownColumns.firstIndex(of: column),
            ListText.editable(shown[index], column)
        else { return }
        table.editColumn(c, row: index, with: nil, select: true)
    }

    /// A cell's field took the keyboard (Return, Tab, a click on it).
    func fieldTookKeyboard(_ field: NSTextField) {
        let r = table.row(for: field)
        let c = table.column(for: field)
        guard shown.indices.contains(r), shownColumns.indices.contains(c),
            ListText.editable(shown[r], shownColumns[c])
        else { return }
        began(row: shown[r].id, column: shownColumns[c], field: field)
    }

    /// A cell edit began: the transport keys go off, played notes fill it.
    func began(row: String, column: ListColumn, field: NSTextField) {
        if let e = editingCell {
            guard e.field !== field else { return }
            // Not reloaded here (inside the new field's becomeFirstResponder):
            // the table reloads when the new edit ends.
            ended(movement: .other, reloading: false)
        }
        editingCell = (row, column, field, field.stringValue)
        model.editing = ListCell(row: row, column: column)
        model.editor.beginTextEditing(textOwner)
        model.document?.activeList = model.slot
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let field = obj.object as? NSTextField, field === editingCell?.field else { return }
        let raw = (obj.userInfo?["NSTextMovement"] as? Int) ?? NSTextMovement.other.rawValue
        ended(movement: NSTextMovement(rawValue: raw) ?? .other)
    }

    /// Editing ended: commit unless cancelled, then move on (Tab and
    /// Shift-Tab to the next editable cell; Return and Escape back to the
    /// table; a click elsewhere keeps its focus).
    func ended(movement: NSTextMovement, cancelled: Bool = false, reloading: Bool = true) {
        guard let e = editingCell else { return }
        editingCell = nil
        model.editing = nil
        model.editor.endTextEditing(textOwner)
        let text = e.field.stringValue
        if !cancelled, text != e.original { model.commit(text, row: e.row, column: e.column) }
        guard reloading else { return }
        reload()
        switch movement {
        case .tab, .backtab: editNext(after: e.row, column: e.column, forward: movement == .tab)
        case .return, .cancel: window?.makeFirstResponder(table)
        default: break
        }
    }

    private func editNext(after row: String, column: ListColumn, forward: Bool) {
        guard let i = shown.firstIndex(where: { $0.id == row }), let c = shownColumns.firstIndex(of: column) else {
            return
        }
        let order = forward ? Array(shownColumns[(c + 1)...]) : Array(shownColumns[..<c].reversed())
        guard let next = order.first(where: { ListText.editable(shown[i], $0) }) else {
            window?.makeFirstResponder(table)
            return
        }
        beginEditing(row: i, column: next)
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)), let e = editingCell else { return false }
        e.field.stringValue = e.original
        e.field.abortEditing()
        ended(movement: .cancel, cancelled: true)
        return true
    }

    // MARK: Keys and focus (Plan 4c Ruling 20)

    /// True when the key was handled here.
    func key(_ code: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        let row = table.selectedRow
        switch code {
        case 36, 76:  // Return, Enter
            guard shown.indices.contains(row) else { return false }
            beginEditing(row: row, column: .position)
            return true
        case 51, 117:  // Delete, Forward Delete
            let ids = table.selectedRowIndexes.compactMap { shown.indices.contains($0) ? shown[$0].id : nil }
            guard !ids.isEmpty else { return false }
            model.delete(ids)
            return true
        case 125 where modifiers.contains(.command):  // ⌘↓
            guard shown.indices.contains(row) else { return false }
            return model.openReference(shown[row].id)
        default:
            return false
        }
    }

    func focusChanged(_ on: Bool) {
        model.editor.listFocused = on
        if on { model.document?.activeList = model.slot }
    }

    @objc func doubleClicked() {
        let r = table.clickedRow
        let c = table.clickedColumn
        guard shown.indices.contains(r), shownColumns.indices.contains(c) else { return }
        let column = shownColumns[c]
        if shown[r].kind == .reference, column == .kind || column == .data1 {
            model.openReference(shown[r].id)
        } else {
            beginEditing(row: r, column: column)
        }
    }

    /// Leaving the window ends an edit unsaved and gives Return back.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        for o in keyObservers { NotificationCenter.default.removeObserver(o) }
        keyObservers = []
        guard newWindow == nil else { return }
        if editingCell != nil { ended(movement: .other, cancelled: true) }
        if window?.firstResponder === table { model.editor.listFocused = false }
    }

    /// The table keeps the keyboard of its window while another window is
    /// key: the list has the keyboard only while its window is key.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let w = window else { return }
        let names = [(NSWindow.didBecomeKeyNotification, true), (NSWindow.didResignKeyNotification, false)]
        for (name, key) in names {
            keyObservers.append(
                NotificationCenter.default.addObserver(forName: name, object: w, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.windowKeyChanged(key) }
                })
        }
    }

    /// A cell edit stays open in a window that is not key, but gives the
    /// keyboard back, as the panel's fields do (Plan 4c Ruling 21); it
    /// takes it again when the window is key again.
    private func windowKeyChanged(_ key: Bool) {
        if window?.firstResponder === table { focusChanged(key) }
        if !key {
            model.editor.endTextEditing(textOwner)
        } else if editingCell != nil {
            model.editor.beginTextEditing(textOwner)
        }
    }
}
