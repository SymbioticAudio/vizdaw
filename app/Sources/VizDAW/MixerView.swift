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

/// One strip: name, kind, slots, sends, output, pan, fader, mute/solo,
/// meter.
struct StripView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @FocusState private var renaming: Bool

    var body: some View {
        VStack(spacing: 4) {
            header
            if let inst = strip.instrument {
                SlotView(model: model, strip: strip, slot: inst, index: nil)
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
                if strip.kind != .master {
                    Toggle("S", isOn: Binding(get: { strip.solo }, set: { _ in model.toggleSolo(strip) }))
                        .toggleStyle(.button)
                        .tint(.yellow)
                }
            }
            .help(strip.soloMuted ? "Silent: other strips are soloed" : "Mute / Solo")
        }
        .padding(6)
        .frame(width: 120)
        .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary.opacity(strip.soloMuted ? 0.3 : 0.6)))
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
                .contextMenu {
                    Button("Rename…") { model.beginRename(strip) }
                    if strip.kind == .bus {
                        Button("Remove Bus") { model.removeBus(strip.id) }
                    }
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
                    value: Binding(get: { strip.pan }, set: { model.setPan(strip.id, $0) }),
                    in: -1...1
                ) { editing in
                    if editing { model.dragStarted("Set Pan", strip: strip.id) } else { model.dragEnded() }
                }
                .controlSize(.mini)
                Text(MixerMath.panLabel(strip.pan))
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
                    get: { MixerMath.faderPosition(send.levelDb) },
                    set: { p in model.editSend(strip.id, index) { $0.levelDb = MixerMath.faderDb(p) } }),
                in: 0...1
            ) { editing in
                if editing { model.dragStarted("Set Send Level", strip: strip.id) } else { model.dragEnded() }
            }
            .controlSize(.mini)
        }
        .contextMenu { Button("Remove Send") { model.removeSend(strip.id, index) } }
        .help("Send to \(model.stripName(send.target)): \(MixerMath.dbLabel(send.levelDb)) dB")
    }
}

/// A vertical fader (a horizontal slider turned); a drag is one undo step.
struct FaderView: View {
    let model: MixerModel
    let strip: StripRow

    var body: some View {
        VStack(spacing: 2) {
            Slider(
                value: Binding(
                    get: { MixerMath.faderPosition(strip.gainDb) },
                    set: { model.setGain(strip.id, db: MixerMath.faderDb($0)) }),
                in: 0...1
            ) { editing in
                if editing { model.dragStarted("Set Fader", strip: strip.id) } else { model.dragEnded() }
            }
            .frame(width: 140)
            .rotationEffect(.degrees(-90))
            .frame(width: 30, height: 140)
            Text(MixerMath.dbLabel(strip.gainDb))
                .font(.caption2.monospacedDigit())
                .onTapGesture(count: 2) { model.setGain(strip.id, db: 0) }
                .help("Double-click: 0 dB")
        }
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
