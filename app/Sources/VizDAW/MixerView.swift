import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The mixer window of document `id` (spec §11; functional, not polished).
struct MixerWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore
    @Environment(\.controlActiveState) private var activeState
    /// This window's focus token (the timer's note target).
    /// (`State` spelled out: the Command Line Tools cannot expand `@State`.)
    private var focusToken = State(initialValue: UUID())

    /// Spelled out: a private stored property makes the memberwise
    /// initializer private under Xcode's compiler.
    init(id: Binding<UUID?>, store: DocumentStore) {
        _id = id
        self.store = store
    }

    var body: some View {
        content
            .background(WindowTag(kind: .mixer, doc: id))
            .onChange(of: KeyDoc(id: id, key: activeState == .key), initial: true) { old, new in
                KeyDoc.changed(store, window: focusToken.wrappedValue, from: old, to: new)
            }
            .onDisappear { if let id { store.blurDocument(id, window: focusToken.wrappedValue) } }
    }

    @ViewBuilder private var content: some View {
        if let id, let mixer = store.mixer(id) {
            MixerView(model: mixer)
                .environment(\.openPluginWindow, { slot in PluginWindows.show(store, doc: id, slot: slot) })
                .focusedSceneValue(\.documentModel, mixer.document)
                .focusedSceneValue(\.documentID, id)
                .navigationTitle("Mixer — \(mixer.document.snapshot.title)")
        } else {
            ContentUnavailableView(
                "No Document", systemImage: "slider.vertical.3",
                description: Text("The document of this mixer is closed."))
        }
    }
}

struct MixerView: View {
    @Bindable var model: MixerModel

    var body: some View {
        VStack(spacing: 0) {
            LearnBanner(learn: model.document.learn, document: model.document)
            strips
        }
        .sheet(isPresented: mapsShown) { LearnMapsSheet(learn: model.document.learn) }
        // The maps sheet goes with the mixer (final review T11 m6).
        .onDisappear { model.document.learn.showsMaps = false }
        .learnAlert(model.document.learn, shown: !model.document.learn.showsMaps)
    }

    /// The MIDI Learn Maps sheet (plan 5f-1 Task 11).
    private var mapsShown: Binding<Bool> {
        let learn = model.document.learn
        return Binding(get: { learn.showsMaps }, set: { learn.showsMaps = $0 })
    }

    private var strips: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 6) {
                ForEach(model.strips, id: \.id) { s in
                    StripView(model: model, strip: s)
                }
                if let master = model.master {
                    Divider()
                    StripView(model: model, strip: master)
                }
            }
            .padding(8)
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Text(model.loadText)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Button("Add Bus") { model.addBus() }
                Menu("Instruments") {
                    Button("New Plugin Instrument…") { model.browser = .instrument }
                    Divider()
                    ForEach(model.destinations, id: \.id) { d in
                        Button("\(d.name): \(d.plugin?.name ?? "MIDI") → Plugin…") {
                            model.browser = .destination(d.id)
                        }
                    }
                }
                .popover(isPresented: instrumentBrowserShown) {
                    PluginBrowserView(model: browserModel(.instrument)) { model.pick($0) }
                }
            }
        }
        .onDisappear { model.endOpenDrag() }
        .task {
            // Meters at 30 Hz (spec §7); the strips follow the document.
            while !Task.isCancelled {
                model.refreshIfNeeded()
                model.pollLevels()
                model.pollAutomation()
                try? await Task.sleep(for: .milliseconds(33))
            }
        }
    }

    /// The instrument browser: for a new instrument or a destination.
    private var instrumentBrowserShown: Binding<Bool> {
        Binding(
            get: { model.browser?.wantsInstrument == true },
            set: { if !$0 { model.browser = nil } })
    }

    private func browserModel(_ kind: PluginKindTag) -> PluginBrowserModel {
        model.browserModel(kind) { PluginBrowserModel(kind: kind) { appPlugins.browser(kind) } }
    }
}

extension View {
    /// The touched ring (plan 5e-2 Ruling 3): shown while `touched`.
    func touchRing(_ touched: Bool, radius: CGFloat = 4) -> some View {
        overlay(
            RoundedRectangle(cornerRadius: radius)
                .stroke(Color.accentColor, lineWidth: 2)
                .opacity(touched ? 1 : 0))
    }
}

/// An automation mode's system colour (Ruling 4).
func automationColor(_ m: AutomationModeRow) -> Color {
    switch AutomationModes.colorName(m) {
    case "green": .green
    case "yellow": .yellow
    case "orange": .orange
    case "red": .red
    case "purple": .purple
    default: .gray
    }
}

/// One strip: name, kind, slots, sends, output, pan, fader, mute/solo,
/// meter.
struct StripView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @FocusState private var renaming: Bool

    var body: some View {
        VStack(spacing: 4) {
            header
            modeMenu
            if let inst = strip.instrument {
                SlotView(model: model, strip: strip, slot: inst, index: nil)
            }
            if let lane = model.inputLane(for: strip) {
                StripInputView(inputs: model.document.inputs, lane: lane)
            }
            inserts
            sends
            output
            panControl
            HStack(alignment: .bottom, spacing: 4) {
                FaderView(model: model, strip: strip)
                MeterView(level: model.meters[strip.id] ?? .silent)
            }
            HStack(spacing: 4) {
                Toggle("M", isOn: Binding(get: { strip.mute }, set: { _ in model.toggleMute(strip) }))
                    .toggleStyle(.button)
                    .tint(.orange)
                    .touchRing(model.isTouched(.stripMute(strip: strip.id)))
                    .contextMenu { LearnItem(model: model, target: .stripMute(strip: strip.id)) }
                if strip.kind != .master {
                    Toggle("S", isOn: Binding(get: { strip.solo }, set: { _ in model.toggleSolo(strip) }))
                        .toggleStyle(.button)
                        .tint(.yellow)
                        .touchRing(model.isTouched(.stripSolo(strip: strip.id)))
                        .contextMenu { LearnItem(model: model, target: .stripSolo(strip: strip.id)) }
                }
            }
            .help(strip.soloMuted ? "Silent: other strips are soloed" : "Mute / Solo")
        }
        .padding(6)
        .frame(width: 120)
        .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary.opacity(strip.soloMuted ? 0.3 : 0.6)))
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(Color.accentColor.opacity(model.isSelected(strip.id) ? 0.25 : 0)))
    }

    @ViewBuilder private var header: some View {
        if model.renamingStrip == strip.id {
            TextField("Name", text: $model.renameDraft)
                .focused($renaming)
                .onSubmit { model.commitRename() }
                .onExitCommand { model.cancelRename() }
                .onAppear { renaming = true }
        } else {
            Text(strip.name)
                .font(.headline)
                .lineLimit(1)
                .onTapGesture(count: 2) { model.beginRename(strip) }
                .onTapGesture { model.select(strip.id, adding: NSEvent.modifierFlags.contains(.command)) }
                .contextMenu {
                    Button("Rename…") { model.beginRename(strip) }
                    Menu("Set All Strips to") {
                        ForEach(AutomationModes.all, id: \.self) { m in
                            Button(AutomationModes.title(m)) { model.setAllModes(m) }
                        }
                    }
                    if strip.kind == .bus {
                        Button("Remove Bus") { model.removeBus(strip.id) }
                    }
                    Divider()
                    Button("MIDI Learn Maps…") { model.document.learn.showsMaps = true }
                    if !strip.freeOutputs.isEmpty {
                        Menu("Add Outputs") {
                            ForEach(strip.freeOutputs, id: \.self) { i in
                                Button("Output \(Int(i) + 1)") { model.addOutput(strip, i) }
                            }
                        }
                    }
                }
        }
        Text(strip.kindLabel)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        let held = model.heldSlots(of: strip)
        if !held.isEmpty {
            Button("Load All") { model.load(held) }
                .controlSize(.small)
                .help("Safe mode kept these plugins off")
        }
    }

    /// The automation mode popup, tinted by the mode.
    private var modeMenu: some View {
        Menu(AutomationModes.title(strip.automation)) {
            ForEach(AutomationModes.all, id: \.self) { m in
                Button {
                    model.setMode(strip.id, m)
                } label: {
                    if m == strip.automation {
                        Label(AutomationModes.title(m), systemImage: "checkmark")
                    } else {
                        Text(AutomationModes.title(m))
                    }
                }
            }
        }
        .tint(automationColor(strip.automation))
        .controlSize(.small)
        .help("Automation: \(AutomationModes.title(strip.automation))")
    }

    private var inserts: some View {
        VStack(spacing: 2) {
            ForEach(Array(strip.inserts.enumerated()), id: \.element.id) { i, slot in
                SlotView(model: model, strip: strip, slot: slot, index: i)
            }
            Button {
                model.browser = .insert(strip: strip.id)
            } label: {
                Image(systemName: "plus")
                    .frame(maxWidth: .infinity)
            }
            .controlSize(.small)
            .help("Add an effect")
            .popover(
                isPresented: Binding(
                    get: { model.browser == .insert(strip: strip.id) },
                    set: { if !$0 { model.browser = nil } })
            ) {
                PluginBrowserView(
                    model: model.browserModel(.effect) {
                        PluginBrowserModel(kind: .effect) { appPlugins.browser(.effect) }
                    }
                ) {
                    model.pick($0)
                }
            }
            .dropDestination(for: String.self) { items, _ in
                drop(items, at: strip.inserts.count)
            }
        }
    }

    private func drop(_ items: [String], at index: Int) -> Bool {
        guard let id = items.first.flatMap(SlotView.slotID) else { return false }
        model.moveInsert(id, to: strip.id, at: index)
        return true
    }

    @ViewBuilder private var sends: some View {
        if strip.kind != .master {
            ForEach(Array(strip.sends.enumerated()), id: \.offset) { i, send in
                SendView(model: model, strip: strip, index: i, send: send)
            }
            let targets = model.targets(for: strip)
            if !targets.isEmpty {
                Menu("Send") {
                    ForEach(targets, id: \.id) { b in
                        Button(b.name) { model.addSend(strip.id, to: b.id) }
                    }
                }
                .controlSize(.small)
            }
        }
    }

    @ViewBuilder private var output: some View {
        if strip.kind != .master {
            Menu("→ \(model.outputName(strip))") {
                Button("Master") { model.setOutput(strip.id, bus: nil) }
                ForEach(model.targets(for: strip), id: \.id) { b in
                    Button(b.name) { model.setOutput(strip.id, bus: b.id) }
                }
            }
            .controlSize(.small)
        }
    }

    @ViewBuilder private var panControl: some View {
        if strip.kind != .master {
            HStack(spacing: 2) {
                Slider(
                    value: Binding(
                        get: { model.pan(strip) },
                        set: { model.move(.stripPan(strip: strip.id), to: ($0 + 1) / 2) }),
                    in: -1...1
                ) { editing in
                    if editing {
                        model.touch(.stripPan(strip: strip.id))
                    } else {
                        model.release(.stripPan(strip: strip.id))
                    }
                }
                .controlSize(.mini)
                .touchRing(model.isTouched(.stripPan(strip: strip.id)))
                .contextMenu { LearnItem(model: model, target: .stripPan(strip: strip.id)) }
                Text(MixerMath.panLabel(model.pan(strip)))
                    .font(.caption2.monospacedDigit())
                    .frame(width: 28)
            }
        }
    }
}

/// One plugin slot: name (dimmed with its reason when a placeholder),
/// actions in its menu, drag to reorder.
struct SlotView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    let slot: SlotRow
    /// Position among the inserts (`nil`: an instrument's own slot).
    let index: Int?
    @Environment(\.openPluginWindow) private var openWindow

    static func slotID(_ text: String) -> UInt64? {
        text.hasPrefix("slot:") ? UInt64(text.dropFirst(5)) : nil
    }

    private var reason: String? {
        if case .placeholder(let r) = slot.status { return r }
        if model.faulted.contains(slot.id) { return "Bypassed: the plugin misbehaved" }
        return nil
    }

    var body: some View {
        HStack(spacing: 3) {
            Circle()
                .fill(color)
                .frame(width: 6, height: 6)
            Text(slot.plugin.name)
                .font(.caption)
                .lineLimit(1)
                .strikethrough(slot.bypass)
                .foregroundStyle(reason == nil ? .primary : .secondary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 4)
        .frame(maxWidth: .infinity, minHeight: 18)
        .background(RoundedRectangle(cornerRadius: 3).fill(.background))
        .help(reason ?? "\(slot.plugin.name) (\(slot.plugin.vendor))")
        .onTapGesture(count: 2) { openWindow(slot.id) }
        .contextMenu { menu }
        .modifier(Reorder(model: model, strip: strip, slot: slot, index: index))
    }

    private var color: Color {
        if model.faulted.contains(slot.id) { return .red }
        switch slot.status {
        case .live: return slot.bypass ? .gray : .green
        case .loading: return .yellow
        case .placeholder: return .orange
        }
    }

    @ViewBuilder private var menu: some View {
        Button("Open") { openWindow(slot.id) }
        if model.held.contains(slot.id) {
            Button("Load") { model.load([slot.id]) }
        }
        Button(slot.bypass ? "Turn On" : "Bypass") { model.toggleBypass(slot) }
        if index != nil {
            if slot.hasSidechainInput {
                Menu("Sidechain") {
                    Button("None") { model.setSidechain(slot.id, source: nil) }
                    ForEach(model.sidechainSources(for: strip), id: \.id) { s in
                        Button((slot.sidechain == s.id ? "✓ " : "") + s.name) {
                            model.setSidechain(slot.id, source: s.id)
                        }
                    }
                }
            }
            Button("Remove") { model.removeInsert(slot.id) }
        }
    }

    /// Inserts drag and drop (an instrument's own slot does not move).
    private struct Reorder: ViewModifier {
        let model: MixerModel
        let strip: StripRow
        let slot: SlotRow
        let index: Int?

        func body(content: Content) -> some View {
            if let index {
                content
                    .draggable("slot:\(slot.id)")
                    .dropDestination(for: String.self) { items, _ in
                        guard let id = items.first.flatMap(SlotView.slotID) else { return false }
                        model.moveInsert(id, to: strip.id, at: index)
                        return true
                    }
            } else {
                content
            }
        }
    }
}

/// One send: target, level (a drag is one undo step), pre/post, on/off.
struct SendView: View {
    let model: MixerModel
    let strip: StripRow
    let index: Int
    let send: SendRow

    var body: some View {
        VStack(spacing: 1) {
            HStack(spacing: 2) {
                Text(model.stripName(send.target))
                    .font(.caption2)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Toggle("Pre", isOn: Binding(get: { send.preFader }, set: { v in model.editSend(strip.id, index) { $0.preFader = v } }))
                    .toggleStyle(.button)
                    .controlSize(.mini)
                Toggle("On", isOn: Binding(get: { send.enabled }, set: { v in model.editSend(strip.id, index) { $0.enabled = v } }))
                    .toggleStyle(.button)
                    .controlSize(.mini)
            }
            Slider(
                value: Binding(
                    get: { MixerMath.faderPosition(model.sendLevelDb(strip, index)) },
                    set: { p in model.move(target, to: AutomationLaw.faderNorm(MixerMath.faderDb(p))) }),
                in: 0...1
            ) { editing in
                if editing { model.touch(target) } else { model.release(target) }
            }
            .controlSize(.mini)
            .touchRing(model.isTouched(target))
        }
        .contextMenu {
            LearnItem(model: model, target: target)
            Button("Remove Send") { model.removeSend(strip.id, index) }
        }
        .help("Send to \(model.stripName(send.target)): \(MixerMath.dbLabel(model.sendLevelDb(strip, index))) dB")
    }

    private var target: AutomationTargetRow { .sendLevel(strip: strip.id, send: MixerModel.sendIndex(index)) }
}

/// A vertical fader (a horizontal slider turned); a drag is one undo step.
struct FaderView: View {
    let model: MixerModel
    let strip: StripRow

    var body: some View {
        VStack(spacing: 2) {
            Slider(
                value: Binding(
                    get: { MixerMath.faderPosition(model.gainDb(strip)) },
                    set: { model.move(target, to: AutomationLaw.faderNorm(MixerMath.faderDb($0))) }),
                in: 0...1
            ) { editing in
                if editing { model.touch(target) } else { model.release(target) }
            }
            .frame(width: 140)
            .rotationEffect(.degrees(-90))
            .frame(width: 30, height: 140)
            .touchRing(model.isTouched(target))
            .contextMenu { LearnItem(model: model, target: target) }
            Text(MixerMath.dbLabel(model.gainDb(strip)))
                .font(.caption2.monospacedDigit())
                .onTapGesture(count: 2) { model.click(target, AutomationLaw.faderNorm(0)) }
                .help("Double-click: 0 dB")
        }
    }

    private var target: AutomationTargetRow { .stripFader(strip: strip.id) }
}

/// "MIDI Learn" in a control's menu (plan 5f-1 Task 11): the next MIDI
/// control moved drives `target`.
struct LearnItem: View {
    let model: MixerModel
    let target: AutomationTargetRow

    var body: some View {
        Button("MIDI Learn") { model.document.learn.learn(target) }
    }
}

/// Peak meter, left and right.
struct MeterView: View {
    let level: MeterLevel

    var body: some View {
        HStack(spacing: 2) {
            bar(level.left)
            bar(level.right)
        }
        .frame(width: 14, height: 140)
    }

    private func bar(_ peak: Float) -> some View {
        GeometryReader { g in
            let fill = MixerMath.meterFill(peak)
            ZStack(alignment: .bottom) {
                Rectangle().fill(.black.opacity(0.6))
                Rectangle()
                    .fill(peak >= 1 ? .red : .green)
                    .frame(height: g.size.height * fill)
            }
        }
    }
}
