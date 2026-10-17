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
    public private(set) var notice: String?
    /// The dragged events an exact-mode panel acts on (the command carries
    /// no ids, so Apply selects them first).
    @ObservationIgnored private var pending: ModifyRequest?
    @ObservationIgnored private var destCache: (version: UInt64, rows: [DestRow])?

    public static let nothingMessage = "Turn on Select or Modify first."
    public static let noLanesMessage = "Select lanes to search first: drag in a ruler or select a block."
    public static let invalidMessage = "Fix the fields marked in red first."
    public static let noNameMessage = "Name the template first."

    /// Modify Again repeats on the selection in place, whatever the last
    /// modify did (ruling F11).
    public static let modifyAgainHelp = "Modify Again changes the selection in place."

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
        notice = nil
        present()
    }

    /// Exact mode: the panel pre-filled with a drag's amounts.
    public func open(_ request: ModifyRequest) {
        task = request.task
        config = request.config
        filter = nil
        pending = request.events == nil ? nil : request
        notice = request.droppedAxis?.message
        present()
    }

    public func close() {
        isOpen = false
        isKey = false
        pending = nil
        notice = nil
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
        // An exact-mode panel acts on the dragged events: select them first.
        let before = editor.selection
        if let p = pending, let events = p.events {
            editor.setSelection(
                SelectionRow(timeRange: before.timeRange, lanes: p.lanes.map(\.ref), events: events))
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
            notice = nil
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
        notice = nil
        drafts = [:]
        invalid = []
    }

    /// Modify > Templates: runs `t` at once on the selection.
    @discardableResult
    public func run(_ t: ModifyTemplateRow) -> Bool {
        load(t)
        return apply()
    }

    @discardableResult
    public func modifyAgain() -> Bool { editor?.edit(.modifyAgain) ?? false }

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
}
