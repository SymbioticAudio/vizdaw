import Foundation
import Observation
import VizDAWCore

/// The select-and-modify panel of one document (parent spec §9): what it is
/// set to, the text typed into it, and what Apply, templates and Modify
/// Again send. The panel window itself is the app's (Plan 4c Ruling 22).
@Observable
@MainActor
public final class SelectModifyModel {
    public weak var document: DocumentModel?
    public var config = SelectModifyConfig.preset(.change)
    public private(set) var task: PanelTask = .change
    public private(set) var isOpen = false
    public var templateName = ""
    /// The opening list's hidden kinds (spec §3.3), else nil.
    public var filter: ViewFilterRow?
    /// Field text as typed, by key ("c<i>.<j>", "m<j>", "map", "offsets").
    public private(set) var drafts: [String: String] = [:]
    /// Drafts that do not parse (drawn red; Apply waits for them).
    public private(set) var invalid: Set<String> = []
    /// Shows the panel window (set by the app).
    @ObservationIgnored public var onPresent: (() -> Void)?
    /// A new random seed per Apply (Plan 4c Ruling 3; tests replace it).
    @ObservationIgnored public var nextSeed: () -> UInt64 = { UInt64.random(in: 0...UInt64.max) }
    /// Why the panel shows what it shows (a dropped drag axis), else nil.
    public var notice: String? { requestIsStale ? nil : storedNotice }
    private var storedNotice: String?
    /// The selection the request was opened over. The request is stale
    /// when the selection moved on, or a dragged event no longer exists
    /// (other snapshots, e.g. plugin loads, leave it alone).
    @ObservationIgnored private var baseline: SelectionRow?
    private var requestIsStale: Bool {
        guard let b = baseline, let e = editor else { return false }
        if e.selection != b { return true }
        guard let p = pending, let ids = p.events, !p.lanes.isEmpty else { return false }
        var present = Set<UInt64>()
        for lane in p.lanes {
            // An unreadable lane (Busy) is not evidence of a missing event.
            guard let rows = e.laneEvents(lane)?.events else { return false }
            present.formUnion(rows.map(\.id))
        }
        return !ids.allSatisfy(present.contains)
    }
    /// The dragged events an exact-mode panel acts on (the command carries
    /// no ids, so Apply selects them first). Observed: the panel says
    /// what Apply acts on.
    private var pending: ModifyRequest?
    @ObservationIgnored private var destCache: (version: UInt64, rows: [DestRow])?

    public static let nothingMessage = "Turn on Select or Modify first."
    public static let noLanesMessage = "Select lanes to search first: drag in a ruler or select a block."
    public static let invalidMessage = "Fix the fields marked in red first."
    public static let notSelectedMessage = "The dragged notes could not be selected."
    public static let noNameMessage = "Name the template first."

    /// Modify Again repeats on the selection in place, whatever the last
    /// modify did (ruling F11).
    public static let modifyAgainHelp = "Modify Again changes the selection in place."

    public static let draggedEventsTarget = "Apply acts on the dragged events."
    public static let selectionTarget = "Apply acts on the current selection."

    /// Apply selects the dragged events of an exact-mode request first.
    public var actsOnDraggedEvents: Bool { pending?.events != nil && !requestIsStale }

    /// What Apply acts on, as the panel says it.
    public var applyTarget: String { actsOnDraggedEvents ? Self.draggedEventsTarget : Self.selectionTarget }

    /// The panel's text fields own the keyboard by these names (Plan 4c
    /// Ruling 21), one per field so moving between fields never clears it.
    public static let textOwnerPrefix = "selectModify."
    public static func textOwner(_ field: String) -> String { textOwnerPrefix + field }

    /// Panel field `field` has the keyboard (`on`) or gave it back.
    public func setTextEditing(_ field: String, _ on: Bool) {
        if on { editor?.beginTextEditing(Self.textOwner(field)) } else { editor?.endTextEditing(Self.textOwner(field)) }
    }

    /// The panel's fields give the keyboard back (it resigned key or closed).
    public func endTextEditing() { editor?.endTextEditing(prefix: Self.textOwnerPrefix) }

    /// Modify > Modify Again's title: the last modify by name.
    public var modifyAgainTitle: String {
        editor?.lastModify().map { "Modify Again: \(ModifyKind.title($0))" } ?? "Modify Again"
    }

    /// Modify Again needs a last modify and no open drag.
    public var canModifyAgain: Bool {
        guard let editor else { return false }
        return editor.lastModify() != nil && editor.gesture == nil
    }

    public init() {}

    var editor: EditorModel? { document?.editor }

    /// The panel window is key: the transport keys are off.
    public var isKey: Bool {
        get { editor?.panelKey ?? false }
        set { editor?.panelKey = newValue }
    }

    /// The meter positions are typed in: the shown sequence's.
    public var meter: [MeterEntryRow] {
        guard let d = document, let seq = d.selectedSeq else { return [] }
        return d.editor.info(seq)?.meter ?? []
    }

    public var templates: [ModifyTemplateRow] { editor?.modifyTemplates() ?? [] }

    public var destinations: [DestRow] {
        guard let editor else { return [] }
        if let c = destCache, c.version == editor.snapshotVersion { return c.rows }
        let rows = (try? editor.session.destinations()) ?? destCache?.rows ?? []
        destCache = (editor.snapshotVersion, rows)
        return rows
    }

    // MARK: Opening

    /// A Modify menu command: the panel set to `task`.
    public func open(_ task: PanelTask, filter: ViewFilterRow? = nil) {
        self.task = task
        config = .preset(task, range: editor?.selection.timeRange)
        self.filter = filter
        pending = nil
        storedNotice = nil
        baseline = nil
        present()
    }

    /// Exact mode: the panel pre-filled with a drag's amounts.
    public func open(_ request: ModifyRequest) {
        task = request.task
        config = request.config
        filter = nil
        pending = request.events == nil ? nil : request
        storedNotice = request.droppedAxis?.message
        baseline = editor?.selection
        present()
    }

    public func close() {
        isOpen = false
        isKey = false
        endTextEditing()
        pending = nil
        storedNotice = nil
        baseline = nil
    }

    private func present() {
        drafts = [:]
        invalid = []
        isOpen = true
        onPresent?()
    }

    // MARK: Running

    /// Runs the configuration on the selection: one undo step when it
    /// modifies, a selection change otherwise.
    @discardableResult
    public func apply() -> Bool {
        guard let editor else { return false }
        guard invalid.isEmpty else {
            editor.status = Self.invalidMessage
            return false
        }
        guard let cmd = config.command() else {
            editor.status = Self.nothingMessage
            return false
        }
        guard editor.gesture == nil else {
            editor.status = EditorModel.gestureOpenMessage
            return false
        }
        if requestIsStale {
            pending = nil
            storedNotice = nil
            baseline = nil
        }
        // An exact-mode panel acts on the dragged events: select them first.
        let before = editor.selection
        if let p = pending, let events = p.events {
            editor.setSelection(
                SelectionRow(timeRange: before.timeRange, lanes: p.lanes.map(\.ref), events: events))
            // A refused selection (Busy, an error) was rolled back and
            // reported: do not run on the old one.
            guard Set(editor.selection.events) == Set(events) else {
                if editor.status == nil { editor.status = Self.notSelectedMessage }
                return false
            }
        }
        if config.rule != nil, config.select.mode != .refine, editor.selection.lanes.isEmpty {
            editor.status = Self.noLanesMessage
            if pending != nil { editor.setSelection(before) }
            return false
        }
        // The core adds no undo step for an apply that changes nothing; a
        // no-op is simply sent and applies as usual.
        let ok = editor.edit(reseeded(cmd), filter: filter)
        if ok {
            pending = nil
            storedNotice = nil
            baseline = nil
        } else if pending != nil {
            editor.setSelection(before)
        }
        return ok
    }

    func reseeded(_ cmd: EditCommand) -> EditCommand {
        guard case let .selectAndModify(rule, mode, .change(t, op, .random(lo, hi, _))?, copy) = cmd else { return cmd }
        return .selectAndModify(
            rule: rule, mode: mode, modify: .change(target: t, op: op, amount: .random(low: lo, high: hi, seed: nextSeed())),
            toCopy: copy)
    }

    @discardableResult
    public func saveTemplate() -> Bool {
        guard let editor else { return false }
        let name = templateName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else {
            editor.status = Self.noNameMessage
            return false
        }
        return editor.edit(.settings(edit: .saveTemplate(template: config.template(named: name))))
    }

    public func deleteTemplate(_ name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        editor?.edit(.settings(edit: .deleteTemplate(name: name)))
    }

    /// The panel shows template `t`.
    public func load(_ t: ModifyTemplateRow) {
        config = SelectModifyConfig(template: t)
        templateName = t.name
        pending = nil
        storedNotice = nil
        baseline = nil
        drafts = [:]
        invalid = []
    }

    /// Modify > Templates: runs `t` at once on the selection.
    @discardableResult
    public func run(_ t: ModifyTemplateRow) -> Bool {
        load(t)
        return apply()
    }

    /// Modify Again acts through the view filter the original Apply had:
    /// the panel's (opened from a list) while the panel is key, else the
    /// active list's while a list is the command surface, else the open
    /// panel's.
    public var modifyAgainFilter: ViewFilterRow? {
        if isOpen && isKey { return filter }
        if let l = document?.listForCommands() { return l.filter.viewFilter }
        return isOpen ? filter : nil
    }

    @discardableResult
    public func modifyAgain() -> Bool { editor?.edit(.modifyAgain, filter: modifyAgainFilter) ?? false }

    // MARK: Editing the select half

    public func addCondition(_ k: ConditionKind) {
        config.select.conditions.append(ConditionRow(test: k.test(range: editor?.selection.timeRange), negate: false))
        config.select.enabled = true
    }

    public func removeCondition(at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions.remove(at: i)
        drafts = [:]
        invalid = []
    }

    public func setComparison(_ c: Comparison, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i] = c.applied(to: config.select.conditions[i])
        drafts = drafts.filter { !$0.key.hasPrefix("c\(i).") }
    }

    public func setKinds(_ kinds: [EventKindRow], at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .kinds(kinds: kinds)
    }

    public func setConditionDest(_ dest: UInt64?, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .dest(dest: dest)
    }

    public func conditionText(_ i: Int, _ j: Int) -> String {
        if let d = drafts["c\(i).\(j)"] { return d }
        guard config.select.conditions.indices.contains(i) else { return "" }
        let t = config.select.conditions[i].test
        let f = t.fields
        let n = t.numbers
        guard f.indices.contains(j), n.indices.contains(j) else { return "" }
        return FieldText.text(n[j], f[j].kind, meter: meter)
    }

    public func setConditionText(_ s: String, _ i: Int, _ j: Int) {
        let key = "c\(i).\(j)"
        drafts[key] = s
        guard config.select.conditions.indices.contains(i) else { return }
        let t = config.select.conditions[i].test
        let f = t.fields
        guard f.indices.contains(j), let v = FieldText.parse(s, f[j].kind, meter: meter) else {
            invalid.insert(key)
            return
        }
        invalid.remove(key)
        var n = t.numbers
        n[j] = v
        config.select.conditions[i].test = t.with(n)
    }

    // MARK: Editing the modify half

    public func setModifyKind(_ k: ModifyKind) {
        config.modify.op = k.row
        config.modify.enabled = true
        drafts = [:]
        invalid = []
    }

    public func setTarget(_ t: ValueTargetRow) {
        guard case let .change(_, op, a) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: ChangeOpKind(op).row(for: t), amount: a)
        drafts = [:]
    }

    public func setOp(_ k: ChangeOpKind) {
        guard case let .change(t, _, a) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: k.row(for: t), amount: k == .scale ? .constant(value: 100) : a)
        drafts = [:]
    }

    public func setAmount(_ k: AmountKind) {
        guard case let .change(t, op, _) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: k.row(seed: nextSeed()))
        drafts = [:]
    }

    public func setCurve(_ c: CurveRow) {
        guard case let .change(t, op, .line(f, to, _)) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: .line(from: f, to: to, curve: c))
    }

    public func setWave(_ w: WaveRow) {
        guard case let .change(t, op, .oscillator(_, lo, hi, p, ph)) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: .oscillator(wave: w, low: lo, high: hi, period: p, phase: ph))
    }

    public func setScale(root: UInt8, scale: Scale) {
        switch config.modify.op {
        case .transposeDiatonic(let steps, _, _):
            config.modify.op = .transposeDiatonic(steps: steps, root: root, mask: scale.mask)
        case .transposeToScale:
            config.modify.op = .transposeToScale(root: root, mask: scale.mask)
        default:
            break
        }
    }

    public func setModifyDest(_ d: UInt64?) { config.modify.op = .setDest(dest: d) }

    public func setSubstitute(_ s: SubstituteRow) {
        config.modify.op = .substitute(to: s)
        drafts = [:]
    }

    public func modifyText(_ j: Int) -> String {
        if let d = drafts["m\(j)"] { return d }
        let f = config.modify.op.fields
        let n = config.modify.op.numbers
        guard f.indices.contains(j), n.indices.contains(j) else { return "" }
        return FieldText.text(n[j], f[j].kind, meter: meter)
    }

    public func setModifyText(_ s: String, _ j: Int) {
        let key = "m\(j)"
        drafts[key] = s
        let op = config.modify.op
        let f = op.fields
        guard f.indices.contains(j), let v = FieldText.parse(s, f[j].kind, meter: meter) else {
            invalid.insert(key)
            return
        }
        invalid.remove(key)
        var n = op.numbers
        n[j] = v
        config.modify.op = op.with(n)
    }

    public var noteMapText: String {
        if let d = drafts["map"] { return d }
        if case .noteMap(let m) = config.modify.op { return NoteMapText.text(m) }
        return ""
    }

    public func setNoteMapText(_ s: String) {
        drafts["map"] = s
        guard let m = NoteMapText.parse(s) else {
            invalid.insert("map")
            return
        }
        invalid.remove("map")
        config.modify.op = .noteMap(map: m)
    }

    public var offsetsText: String {
        if let d = drafts["offsets"] { return d }
        if case .quantize(_, _, let o) = config.modify.op { return OffsetsText.text(o) }
        return ""
    }

    public func setOffsetsText(_ s: String) {
        drafts["offsets"] = s
        guard let o = OffsetsText.parse(s), case let .quantize(g, st, _) = config.modify.op else {
            invalid.insert("offsets")
            return
        }
        invalid.remove("offsets")
        config.modify.op = .quantize(grid: g, strength: st, offsets: o)
    }

    // MARK: Strip Chart lines and automation targets (Plan 4d Ruling 24)

    @ObservationIgnored private var targetsCache: (version: UInt64, names: StripNames, menus: [StripMenu])?

    private func targets() -> (names: StripNames, menus: [StripMenu]) {
        guard let editor else { return (StripNames(), []) }
        if let c = targetsCache, c.version == editor.snapshotVersion { return (c.names, c.menus) }
        // Busy throws: keep what we had (or nothing) and retry on the next read.
        guard let t = try? AutomationTargets.load(editor.session) else {
            return targetsCache.map { ($0.names, $0.menus) } ?? (StripNames(), [])
        }
        targetsCache = (editor.snapshotVersion, t.names, t.menus)
        return t
    }

    /// Target names for the pickers.
    public var automationNames: StripNames { targets().names }
    /// The automation targets, grouped as the Strip Chart's row menu groups them.
    public var automationMenus: [StripMenu] { targets().menus }

    public func setConditionTarget(_ t: AutoTarget, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .automation(target: t.row)
    }

    public func setLineTarget(_ lane: StripLane) {
        guard case let .line(_, op, l) = config.modify.op else { return }
        let keep = op != .legato || lane == .duration
        config.modify.op = .line(target: lane.lineTarget, op: keep ? op : .set, line: l)
        drafts = [:]
        invalid = []
    }

    public func setLineOp(_ op: LineOpRow) {
        guard case let .line(t, _, l) = config.modify.op else { return }
        let lengths = t == .value(target: .duration)
        config.modify.op = .line(target: t, op: op == .legato && !lengths ? .set : op, line: l)
        drafts = [:]
        invalid = []
    }

    /// A new shape with its defaults; Free becomes a two-point line from
    /// Start to End (a drawn line keeps its points).
    public func setLineShape(_ k: ShapeKind) {
        guard case let .line(t, op, l) = config.modify.op else { return }
        var line = l
        switch k {
        case .free:
            if case .free = l.shape { return }
            line.shape = .free(points: [
                LinePointRow(tick: l.from, value: l.start), LinePointRow(tick: l.to, value: l.end),
            ])
        default:
            var s = StripChartSettings()
            s.shape = k
            line.shape = s.shapeRow(seed: nextSeed())
        }
        config.modify.op = .line(target: t, op: op, line: line)
        drafts = [:]
        invalid = []
    }

    public func setLineCurve(_ c: CurveRow) {
        guard case .line(let t, let op, var l) = config.modify.op, case .curved = l.shape else { return }
        l.shape = .curved(curve: c)
        config.modify.op = .line(target: t, op: op, line: l)
    }

    public func setLineWave(_ w: WaveRow) {
        guard case .line(let t, let op, var l) = config.modify.op, case let .oscillator(_, p, ph) = l.shape else {
            return
        }
        l.shape = .oscillator(wave: w, period: p, phase: ph)
        config.modify.op = .line(target: t, op: op, line: l)
    }

    public func setLinePeriodKind(_ k: PeriodKind) {
        guard case .line(let t, let op, var l) = config.modify.op, case let .oscillator(w, _, ph) = l.shape else {
            return
        }
        var s = StripChartSettings()
        s.period = k
        l.shape = .oscillator(wave: w, period: s.periodRow, phase: ph)
        config.modify.op = .line(target: t, op: op, line: l)
        drafts = [:]
        invalid = []
    }
}

extension ShapeKind {
    /// The kind of a line's shape (the panel's picker).
    public init(_ s: LineShapeRow) {
        switch s {
        case .straight: self = .straight
        case .curved: self = .curved
        case .flat: self = .flat
        case .free: self = .free
        case .random: self = .random
        case .oscillator: self = .oscillator
        }
    }
}
