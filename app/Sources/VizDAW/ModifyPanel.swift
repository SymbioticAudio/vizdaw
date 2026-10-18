import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The floating select-and-modify panels, one per document (Plan 4c
/// Ruling 22). A closed panel is kept and shown again.
@MainActor
enum ModifyPanels {
    private static var open: [UUID: ModifyPanelController] = [:]

    static func show(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc) else { return }
        let c = open[doc] ?? ModifyPanelController(store: store, model: model, doc: doc)
        open[doc] = c
        c.showWindow(nil)
        c.window?.makeKeyAndOrderFront(nil)
    }

    /// The document is closing.
    static func close(_ doc: UUID) {
        open.removeValue(forKey: doc)?.close()
    }
}

@MainActor
final class ModifyPanelController: NSWindowController, NSWindowDelegate {
    private let store: DocumentStore
    private let doc: UUID

    init(store: DocumentStore, model: DocumentModel, doc: UUID) {
        self.store = store
        self.doc = doc
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 620, height: 460),
            styleMask: [.titled, .closable, .resizable, .utilityWindow], backing: .buffered, defer: false)
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = true
        panel.becomesKeyOnlyIfNeeded = false
        panel.isReleasedWhenClosed = false
        panel.title = "Select and Modify — \(model.snapshot.title)"
        super.init(window: panel)
        panel.contentView = NSHostingView(
            rootView: SelectModifyPanelView(model: model.selectModify, editor: model.editor))
        panel.delegate = self
        panel.center()
    }

    required init?(coder: NSCoder) { nil }

    // The store keeps the panel's focus (menu command target, transport
    // keys, text owners) in step with the window: never stale after a
    // resign or a close, whether the user or the document closed it.
    func windowDidBecomeKey(_ notification: Notification) { store.panelBecameKey(doc) }

    func windowDidResignKey(_ notification: Notification) { store.panelResignedKey(doc) }

    func windowWillClose(_ notification: Notification) { store.panelClosed(doc) }
}

struct SelectModifyPanelView: View {
    @Bindable var model: SelectModifyModel
    let editor: EditorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Picker("Task", selection: Binding(get: { model.task }, set: { model.open($0, filter: model.filter) })) {
                    ForEach(PanelTask.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .frame(width: 260)
                Spacer()
                Menu("Templates") {
                    if model.templates.isEmpty { Text("No Templates") }
                    ForEach(model.templates, id: \.name) { t in
                        Button("Load “\(t.name)”") { model.load(t) }
                    }
                    if !model.templates.isEmpty {
                        Divider()
                        ForEach(model.templates, id: \.name) { t in
                            Button("Delete “\(t.name)”") { model.deleteTemplate(t.name) }
                        }
                    }
                }
                .frame(width: 120)
            }
            Toggle("Select", isOn: $model.config.select.enabled).font(.headline)
            if model.config.select.enabled { SelectHalfView(model: model) }
            Divider()
            Toggle("Modify", isOn: $model.config.modify.enabled).font(.headline)
            if model.config.modify.enabled { ModifyHalfView(model: model) }
            Divider()
            if let notice = model.notice {
                Label(notice, systemImage: "info.circle")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            HStack {
                TextField("Template name", text: $model.templateName)
                    .panelTextOwner(model, "templateName")
                    .frame(width: 180)
                Button("Save as Template") { model.saveTemplate() }
                Spacer()
                Text(model.applyTarget)
                    .font(.caption)
                    .foregroundStyle(model.actsOnDraggedEvents ? Color.accentColor : Color.secondary)
                Button("Modify Again") { model.modifyAgain() }
                    .disabled(!model.canModifyAgain)
                    .help(SelectModifyModel.modifyAgainHelp)
                Button("Apply") { model.apply() }.keyboardShortcut(.defaultAction)
            }
            StatusLine(editor: editor)
        }
        .padding(12)
        .frame(minWidth: 580, minHeight: 320, alignment: .topLeading)
    }
}

struct SelectHalfView: View {
    @Bindable var model: SelectModifyModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Result", selection: $model.config.select.mode) {
                    Text("Replace the Selection").tag(SelectModeRow.select)
                    Text("Add to the Selection").tag(SelectModeRow.add)
                    Text("Refine the Selection").tag(SelectModeRow.refine)
                }
                .frame(width: 280)
                Picker("Match", selection: $model.config.select.matchAll) {
                    Text("All Conditions").tag(true)
                    Text("Any Condition").tag(false)
                }
                .frame(width: 220)
            }
            ForEach(Array(model.config.select.conditions.indices), id: \.self) { i in
                ConditionRowView(model: model, index: i)
            }
            Menu("Add Condition") {
                ForEach(ConditionKind.allCases, id: \.self) { k in
                    Button(k.title) { model.addCondition(k) }
                }
            }
            .frame(width: 160)
        }
    }
}

struct ConditionRowView: View {
    let model: SelectModifyModel
    let index: Int

    var body: some View {
        if model.config.select.conditions.indices.contains(index) {
            let c = model.config.select.conditions[index]
            HStack(spacing: 6) {
                Text(ConditionKind(c.test).title).frame(width: 120, alignment: .leading)
                Picker("", selection: Binding(get: { Comparison.of(c) }, set: { model.setComparison($0, at: index) })) {
                    ForEach(Comparison.choices(for: c.test), id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 90)
                .accessibilityLabel("Comparison")
                switch c.test {
                case .kinds(let kinds):
                    KindsMenu(model: model, index: index, kinds: kinds)
                case .dest(let dest):
                    DestPicker(destinations: model.destinations, selection: dest) { model.setConditionDest($0, at: index) }
                case .automation(let target):
                    AutomationTargetPicker(model: model, selection: AutoTarget(target)) {
                        model.setConditionTarget($0, at: index)
                    }
                default:
                    ForEach(Array(c.test.fields.enumerated()), id: \.offset) { j, f in
                        TextField(
                            f.label,
                            text: Binding(get: { model.conditionText(index, j) }, set: { model.setConditionText($0, index, j) })
                        )
                        .panelTextOwner(model, "c\(index).\(j)")
                        .frame(width: 84)
                        .foregroundStyle(model.invalid.contains("c\(index).\(j)") ? Color.red : Color.primary)
                        .accessibilityLabel("\(ConditionKind(c.test).title) \(f.label)")
                    }
                }
                Spacer()
                Button {
                    model.removeCondition(at: index)
                } label: {
                    Image(systemName: "minus.circle")
                }
                .buttonStyle(.borderless)
                .help("Remove Condition")
                .accessibilityLabel("Remove Condition")
            }
        }
    }
}

struct KindsMenu: View {
    let model: SelectModifyModel
    let index: Int
    let kinds: [EventKindRow]
    static let all: [EventKindRow] = [
        .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx, .text, .reference, .audioRegion,
        .automation,
    ]

    var body: some View {
        Menu(kinds.isEmpty ? "No Kind" : kinds.map { ListKind($0).title }.joined(separator: ", ")) {
            ForEach(Self.all, id: \.self) { k in
                Toggle(
                    ListKind(k).title,
                    isOn: Binding(
                        get: { kinds.contains(k) },
                        set: { on in model.setKinds(on ? kinds + [k] : kinds.filter { $0 != k }, at: index) }))
            }
        }
        .frame(width: 220)
    }
}

struct DestPicker: View {
    let destinations: [DestRow]
    let selection: UInt64?
    let set: (UInt64?) -> Void

    var body: some View {
        Picker("", selection: Binding(get: { selection }, set: { set($0) })) {
            Text("No Destination").tag(UInt64?.none)
            ForEach(destinations, id: \.id) { d in Text(d.name).tag(Optional(d.id)) }
        }
        .labelsHidden()
        .frame(width: 180)
        .accessibilityLabel("Destination")
    }
}

/// Substitute's choices (Plan 4c Ruling 5).
enum SubstituteChoice: String, CaseIterable {
    case controller, pitchBend, channelPressure, pitch

    init(_ s: SubstituteRow) {
        switch s {
        case .controller: self = .controller
        case .pitchBend: self = .pitchBend
        case .channelPressure: self = .channelPressure
        case .pitch: self = .pitch
        }
    }

    var title: String {
        switch self {
        case .controller: "Controller"
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .pitch: "One Pitch"
        }
    }

    var row: SubstituteRow {
        switch self {
        case .controller: .controller(index: 1)
        case .pitchBend: .pitchBend
        case .channelPressure: .channelPressure
        case .pitch: .pitch(pitch: 36)
        }
    }
}

struct ModifyHalfView: View {
    @Bindable var model: SelectModifyModel

    private var op: ModifyRow { model.config.modify.op }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Operation", selection: Binding(get: { ModifyKind(op) }, set: { model.setModifyKind($0) })) {
                    ForEach(ModifyKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .frame(width: 260)
                Picker("Apply", selection: $model.config.modify.toCopy) {
                    Text("In Place").tag(false)
                    Text("To a Copy").tag(true)
                }
                .pickerStyle(.segmented)
                .frame(width: 200)
            }
            HStack(spacing: 6) {
                specials
                ForEach(Array(op.fields.enumerated()), id: \.offset) { j, f in
                    TextField(f.label, text: Binding(get: { model.modifyText(j) }, set: { model.setModifyText($0, j) }))
                        .panelTextOwner(model, "m\(j)")
                        .frame(width: 84)
                        .foregroundStyle(model.invalid.contains("m\(j)") ? Color.red : Color.primary)
                        .accessibilityLabel(f.label)
                }
                Spacer()
            }
        }
    }

    @ViewBuilder private var specials: some View {
        switch op {
        case let .transposeDiatonic(_, root, mask), let .transposeToScale(root, mask):
            Picker("Root", selection: Binding(get: { root }, set: { model.setScale(root: $0, scale: Scale.of(mask: mask) ?? .major) })) {
                ForEach(0..<12, id: \.self) { i in Text(ScaleMath.rootNames[i]).tag(UInt8(i)) }
            }
            .frame(width: 110)
            Picker("Scale", selection: Binding(get: { Scale.of(mask: mask) ?? .major }, set: { model.setScale(root: root, scale: $0) })) {
                ForEach(Scale.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .frame(width: 190)
        case .noteMap:
            TextField("C2>D2, E2>F#2", text: Binding(get: { model.noteMapText }, set: { model.setNoteMapText($0) }))
                .panelTextOwner(model, "map")
                .frame(width: 260)
                .foregroundStyle(model.invalid.contains("map") ? Color.red : Color.primary)
                .accessibilityLabel("Note Map")
        case .quantize:
            TextField("Groove offsets", text: Binding(get: { model.offsetsText }, set: { model.setOffsetsText($0) }))
                .panelTextOwner(model, "offsets")
                .frame(width: 140)
                .foregroundStyle(model.invalid.contains("offsets") ? Color.red : Color.primary)
                .help("Groove offsets in ticks, one per grid line; empty for a plain grid")
        case let .change(target, changeOp, amount):
            Picker("", selection: Binding(get: { target }, set: { model.setTarget($0) })) {
                Text("Velocity").tag(ValueTargetRow.velocity)
                Text("Release Velocity").tag(ValueTargetRow.releaseVelocity)
                Text("Length").tag(ValueTargetRow.duration)
                Text("Value").tag(ValueTargetRow.value)
            }
            .labelsHidden()
            .frame(width: 140)
            .accessibilityLabel("Value to Change")
            Picker("", selection: Binding(get: { ChangeOpKind(changeOp) }, set: { model.setOp($0) })) {
                ForEach(ChangeOpKind.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 120)
            .accessibilityLabel("Operation")
            switch changeOp {
            case .set, .scale, .add:
                Picker("", selection: Binding(get: { AmountKind(amount) }, set: { model.setAmount($0) })) {
                    ForEach(AmountKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 130)
                .accessibilityLabel("Amount")
            default:
                EmptyView()
            }
            switch amount {
            case .line(_, _, let curve):
                Picker("", selection: Binding(get: { curve }, set: { model.setCurve($0) })) {
                    Text("Straight").tag(CurveRow.straight)
                    Text("Ease In").tag(CurveRow.easeIn)
                    Text("Ease Out").tag(CurveRow.easeOut)
                    Text("S-Curve").tag(CurveRow.sCurve)
                }
                .labelsHidden()
                .frame(width: 110)
                .accessibilityLabel("Curve")
            case .oscillator(let wave, _, _, _, _):
                Picker("", selection: Binding(get: { wave }, set: { model.setWave($0) })) {
                    Text("Sine").tag(WaveRow.sine)
                    Text("Square").tag(WaveRow.square)
                    Text("Triangle").tag(WaveRow.triangle)
                    Text("Saw").tag(WaveRow.saw)
                }
                .labelsHidden()
                .frame(width: 100)
                .accessibilityLabel("Wave")
            default:
                EmptyView()
            }
        case let .line(target, lineOp, line):
            LineModifyEditor(model: model, target: target, op: lineOp, line: line)
        case .setDest(let dest):
            DestPicker(destinations: model.destinations, selection: dest) { model.setModifyDest($0) }
        case .substitute(let to):
            Picker("", selection: Binding(get: { SubstituteChoice(to) }, set: { model.setSubstitute($0.row) })) {
                ForEach(SubstituteChoice.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 150)
            .accessibilityLabel("Substitute With")
        default:
            EmptyView()
        }
    }
}

/// A panel text field owns the keyboard while it edits in the key panel
/// (Plan 4c Ruling 21): Space and Return type there, not into the
/// transport. A field torn down mid-edit gives the keyboard back.
struct PanelTextOwner: ViewModifier {
    let model: SelectModifyModel
    let field: String
    @FocusState private var focused: Bool
    @Environment(\.controlActiveState) private var windowState

    func body(content: Content) -> some View {
        content
            .focused($focused)
            .onChange(of: focused && windowState == .key) { _, editing in model.setTextEditing(field, editing) }
            .onDisappear { model.setTextEditing(field, false) }
    }
}

extension View {
    func panelTextOwner(_ model: SelectModifyModel, _ field: String) -> some View {
        modifier(PanelTextOwner(model: model, field: field))
    }
}
