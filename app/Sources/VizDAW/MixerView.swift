import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

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
            VStack(spacing: 0) {
                DocumentTransportBar(model: mixer.document, docID: id, showsViews: false)
                Divider()
                MixerView(model: mixer, placement: .window)
            }
            .environment(\.openPluginWindow, { slot in PluginWindows.show(store, doc: id, slot: slot) })
            .focusedSceneValue(\.documentModel, mixer.document)
            .focusedSceneValue(\.documentID, id)
            .navigationTitle("Mixer — \(mixer.document.snapshot.title)")
        } else {
            ContentUnavailableView(
                "No Document", icon: .mixer,
                description: Text("The document of this mixer is closed."))
        }
    }
}

/// The mixer in one of its placements (plan 7a Ruling 8): its window, the
/// bottom pane, the Mix view. Its bar, popovers, rename field and maps
/// sheet show only in the placement that opened them.
struct MixerView: View {
    @Bindable var model: MixerModel
    let placement: MixerPlacement
    @Environment(\.controlActiveState) private var activeState
    @Environment(\.symbioticTheme) private var theme

    /// The mixer window always polls; the Mix view and the bottom pane only
    /// while their window is key or the document plays or records, as the
    /// left strip (Task 8 review M2): a background project left in Mix does
    /// not read levels at 30 Hz.
    private var polls: Bool { pollKey.key || pollKey.playing }

    private var pollKey: PollKey {
        PollKey(
            key: placement == .window || activeState == .key,
            playing: model.document.isPlaying || model.document.record.isRecording)
    }

    var body: some View {
        VStack(spacing: 0) {
            MixerBar(model: model, placement: placement)
            Divider()
            LearnBanner(learn: model.document.learn, document: model.document)
            strips
        }
        .sheet(isPresented: mapsShown) { LearnMapsSheet(learn: model.document.learn) }
        .onAppear { model.appeared(placement) }
        .onDisappear {
            // The maps sheet goes with its placement (final review T11 m6);
            // open drags end when the last placement goes.
            model.document.learn.closeMaps(from: placement)
            model.disappeared(placement)
        }
        .modifier(UnhostedLearnAlert(learn: model.document.learn))
        .task(id: pollKey) {
            // Meters at 30 Hz (spec §7): one poll per frame, however many
            // placements show (`tick`); the strips follow the document.
            guard polls else { return }
            while !Task.isCancelled {
                model.tick(now: ProcessInfo.processInfo.systemUptime)
                try? await Task.sleep(for: .milliseconds(33))
            }
        }
    }

    /// The MIDI Learn Maps sheet (plan 5f-1 Task 11), in this placement only.
    private var mapsShown: Binding<Bool> {
        let learn = model.document.learn
        let p = placement
        return Binding(get: { learn.showsMaps(in: p) }, set: { if !$0 { learn.closeMaps(from: p) } })
    }

    /// The strips, each given the row's height (Ruling 14): the fader takes
    /// what the rest leaves. When a strip needs more than the row has
    /// (`StripLayout.split`'s `scrolls`) it is taller than the row, and the
    /// row scrolls on both axes; otherwise only across.
    private var strips: some View {
        GeometryReader { g in
            let height = max(g.size.height - 2 * theme.spacing.s, 0)
            ScrollView([.horizontal, .vertical]) {
                // One grid, so the strips share their rows (fix round 1).
                ChannelStrips(
                    model: model, strips: model.strips + (model.master.map { [$0] } ?? []),
                    dividerBefore: model.master == nil ? nil : model.strips.count, height: height)
                .padding(theme.spacing.s)
                .frame(minWidth: g.size.width, minHeight: g.size.height, alignment: .topLeading)
            }
        }
        .environment(\.mixerPlacement, placement)
    }
}

/// The mixer's own bar (it was the window's toolbar; a toolbar item in a
/// pane would land in the document window's): the load, Add Bus and the
/// Instruments menu with its popover (Ruling 8).
struct MixerBar: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: MixerModel
    let placement: MixerPlacement

    var body: some View {
        HStack(spacing: theme.spacing.m) {
            Spacer(minLength: 0)
            // AppKit, so the channel's tree finds them (final review I1).
            AppKitLabel(model.loadText, font: theme.digitsFont(theme.type.caption), secondary: true)
                .fixedSize()
            AppKitButton("Add Bus", help: "Add a bus") { model.addBus() }
                .fixedSize()
            Menu("Instruments") {
                Button("New Plugin Instrument…") { model.openBrowser(.instrument, in: placement) }
                Divider()
                ForEach(model.destinations, id: \.id) { d in
                    Button("\(d.name): \(d.plugin?.name ?? "MIDI") → Plugin…") {
                        model.openBrowser(.destination(d.id), in: placement)
                    }
                }
            }
            .fixedSize()
            .popover(isPresented: instrumentBrowserShown) {
                PluginBrowserView(model: browserModel(.instrument)) { model.pick($0) }
            }
        }
        .padding(.horizontal, theme.spacing.s)
        .padding(.vertical, theme.spacing.xs)
    }

    /// The instrument browser, for a new instrument or a destination, in
    /// the placement that opened it.
    private var instrumentBrowserShown: Binding<Bool> {
        let p = placement
        return Binding(
            get: { model.showsBrowser(in: p) && model.browser?.wantsInstrument == true },
            set: { if !$0 { model.browser = nil } })
    }

    private func browserModel(_ kind: PluginKindTag) -> PluginBrowserModel {
        model.browserModel(kind) { PluginBrowserModel(kind: kind) { appPlugins.browser(kind) } }
    }
}

extension View {
    /// The touched ring (plan 5e-2 Ruling 3): shown while `touched`, in the
    /// theme's focus ring colour and small radius.
    func touchRing(_ touched: Bool) -> some View { modifier(TouchRing(touched: touched)) }
}

private struct TouchRing: ViewModifier {
    let touched: Bool
    @Environment(\.symbioticTheme) private var theme

    func body(content: Content) -> some View {
        content.overlay(
            RoundedRectangle(cornerRadius: theme.radius.s)
                .stroke(theme.surface.focusRing.color, lineWidth: 2)
                .opacity(touched ? 1 : 0))
    }
}

/// A refused learn's alert, unless the document window's shell hosts the
/// one alert (`\.learnAlertHosted`, Ruling 8): the mixer window and torn
/// windows keep theirs.
struct UnhostedLearnAlert: ViewModifier {
    let learn: LearnModel?
    @Environment(\.learnAlertHosted) var hosted

    func body(content: Content) -> some View {
        if let learn, !hosted {
            content.learnAlert(learn, shown: !learn.showsMaps)
        } else {
            content
        }
    }
}

/// One plugin slot: name (dimmed with its reason when a placeholder),
/// actions in its menu, drag to reorder.
struct SlotView: View {
    @Environment(\.symbioticTheme) private var theme
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
        HStack(spacing: theme.spacing.xs) {
            Circle()
                .fill(color)
                .frame(width: 6, height: 6)
            Text(slot.plugin.name)
                .font(theme.type.caption.font)
                .lineLimit(1)
                .strikethrough(slot.bypass)
                .foregroundStyle(reason == nil ? theme.text.primary.color : theme.text.secondary.color)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, theme.spacing.xs)
        .frame(maxWidth: .infinity, minHeight: 20)
        .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(theme.surface.inset.color))
        .overlay(RoundedRectangle(cornerRadius: theme.radius.s).strokeBorder(theme.surface.separator.color))
        .contentShape(Rectangle())
        .help(reason ?? "\(slot.plugin.name) (\(slot.plugin.vendor))")
        .onTapGesture(count: 2) { openWindow(slot.id) }
        .contextMenu { menu }
        .modifier(Reorder(model: model, strip: strip, slot: slot, index: index))
    }

    /// The slot dot (plan 7b Ruling 13).
    private var color: Color { Self.dot(slot, model: model, theme: theme) }

    /// A slot's dot: faulted, live, bypassed, loading or a placeholder (the
    /// EQ slot's too).
    static func dot(_ slot: SlotRow, model: MixerModel, theme: Theme) -> Color {
        if model.faulted.contains(slot.id) { return theme.state.error.color }
        switch slot.status {
        case .live: return slot.bypass ? theme.text.disabled.color : theme.state.play.color
        case .loading: return theme.state.offline.color
        case .placeholder: return theme.state.warning.color
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

/// "MIDI Learn" in a control's menu (plan 5f-1 Task 11): the next MIDI
/// control moved drives `target`.
struct LearnItem: View {
    let model: MixerModel
    let target: AutomationTargetRow

    var body: some View {
        Button("MIDI Learn") { model.document.learn.learn(target) }
    }
}
