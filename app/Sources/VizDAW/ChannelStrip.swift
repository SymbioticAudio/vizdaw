import AppKit
import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// One channel strip (plan 7b Ruling 14), 120 points wide, in every
/// placement: the left pane, the bottom pane's mixer, the Mix view and the
/// mixer window. Top to bottom: the colour bar and name, the instrument
/// slot and input, the EQ slot, the inserts, the sends, the output, the
/// pan, the fader with its scale and meter, the level, M S R I and the
/// automation mode. `height` is the placement's: the fader's throw takes
/// what the rest leaves (`StripLayout.split`), from 160 to 480, and when
/// even 160 does not fit the strip is taller than `height` and scrolls
/// (the left pane's strip itself, the mixer's row in its two-axis scroll
/// view). A mixer shows its strips through `ChannelStrips`, which lines
/// their rows up.
struct ChannelStrip: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    let height: CGFloat
    @Environment(\.mixerPlacement) private var placement

    var body: some View {
        let one = ChannelStrips(model: model, strips: [strip], dividerBefore: nil, height: height)
        if placement == .strip {
            // The left pane's own scrolling (Ruling 14): it scrolls only when
            // the content is taller than the pane, which `split` decides.
            ScrollView(.vertical) { one }
        } else {
            one
        }
    }
}

/// A mixer's strips side by side, sharing their rows (fix round 1): each
/// row above the fader is as tall as the tallest strip's, and a strip
/// without it (no input, no sends) keeps the space empty, so every fader,
/// level box and button row lines up (`StripGrid`). `dividerBefore` puts a
/// divider before that strip (master).
struct ChannelStrips: View {
    @Bindable var model: MixerModel
    let strips: [StripRow]
    let dividerBefore: Int?
    let height: CGFloat

    var body: some View {
        StripGridLayout(height: height, columns: strips.count, dividerBefore: dividerBefore) {
            ForEach(Array(strips.enumerated()), id: \.element.id) { i, s in
                StripPanel(model: model, strip: s)
                    .layoutValue(key: StripCellKey.self, value: StripCell(column: i, part: .panel))
            }
            if dividerBefore != nil {
                StripDivider().layoutValue(key: StripCellKey.self, value: StripCell(column: 0, part: .divider))
            }
            ForEach(Array(strips.enumerated()), id: \.element.id) { i, s in
                StripCells(model: model, strip: s, column: i)
            }
        }
    }
}

/// The line before master (a `Divider` outside a stack draws across).
private struct StripDivider: View {
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        Rectangle().fill(theme.surface.separator.color).accessibilityHidden(true)
    }
}

/// A grid child's place in the strips' grid (`StripGrid.Cell`, without
/// its height: the layout measures it).
typealias StripCell = StripGrid.Cell

private struct StripCellKey: LayoutValueKey {
    static let defaultValue = StripCell(column: 0, part: .panel)
}

/// A strip's cells, each tagged with its column and row.
private struct StripCells: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    let column: Int

    var body: some View {
        row(.header) { StripHeader(model: model, strip: strip) }
        if let inst = strip.instrument {
            row(.instrument) { SlotView(model: model, strip: strip, slot: inst, index: nil) }
        }
        if let lane = model.inputLane(for: strip) {
            row(.input) { StripInputView(inputs: model.document.inputs, lane: lane) }
        }
        row(.eq, band: .dark) { EqSlotView(model: model, strip: strip) }
        row(.inserts, band: .dark) { StripInserts(model: model, strip: strip) }
        if strip.kind != .master {
            if !strip.sends.isEmpty || !model.targets(for: strip).isEmpty {
                row(.sends, band: .light) { SendKnobRow(model: model, strip: strip) }
            }
            row(.output) { StripOutputMenu(model: model, strip: strip) }
            row(.pan, band: .dark) { StripPan(model: model, strip: strip) }
        }
        StripFaderSection(model: model, strip: strip).cell(column, .fader)
        StripLevelBox(model: model, strip: strip).cell(column, .level)
        StripButtons(model: model, strip: strip).cell(column, .buttons)
        StripModeMenu(model: model, strip: strip).cell(column, .mode)
    }

    /// A row above the fader: it fills the row's shared height, its band
    /// (in a banded theme) with it.
    private func row(_ kind: StripRowKind, band: StripBandShade? = nil, @ViewBuilder _ content: () -> some View)
        -> some View
    {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .stripBand(band)
            .cell(column, .row(kind))
    }
}

extension View {
    fileprivate func cell(_ column: Int, _ part: StripGrid.Part) -> some View {
        layoutValue(key: StripCellKey.self, value: StripCell(column: column, part: part))
    }
}

/// A strip's panel: raised, dimmed while other strips solo, with the
/// theme's relief gradient over it when the theme has one (Console), and
/// its edge (the accent when selected).
private struct StripPanel: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.radius.m)
        let selected = model.isSelected(strip.id)
        ZStack {
            shape.fill(theme.surface.raised.color.opacity(strip.soloMuted ? 0.5 : 1))
            if theme.relief.panelTop.fallbackComponents.alpha > 0 || theme.relief.panelBottom.fallbackComponents.alpha > 0 {
                shape.fill(
                    LinearGradient(
                        colors: [theme.relief.panelTop.color, theme.relief.panelBottom.color],
                        startPoint: .top, endPoint: .bottom))
            }
            shape.strokeBorder(
                selected ? theme.accent.accent.color : theme.surface.separator.color, lineWidth: selected ? 2 : 1)
        }
        .accessibilityHidden(true)
    }
}

/// The strips' grid (Ruling 14, fix round 1): it measures the rows above
/// the fader and places every child where `StripGrid.place` puts it, so
/// the strips share their rows and every fader, level box and button row
/// lines up. Every strip is `StripLayout.contentHeight` tall: `height`
/// when it fits, taller (scrolling) when it does not.
private struct StripGridLayout: Layout {
    let height: CGFloat
    let columns: Int
    let dividerBefore: Int?

    private func cells(_ subviews: Subviews) -> [StripCell] {
        let inner = StripLayout.width - 2 * StripLayout.sidePadding
        return subviews.map { v in
            var c = v[StripCellKey.self]
            if case .row = c.part {
                c.measured = v.sizeThatFits(ProposedViewSize(width: inner, height: nil)).height
            }
            return c
        }
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        StripGrid.place(cells(subviews), height: height, dividerBefore: dividerBefore).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let frames = StripGrid.place(cells(subviews), height: height, dividerBefore: dividerBefore).frames
        for (v, f) in zip(subviews, frames) {
            v.place(
                at: CGPoint(x: bounds.minX + f.minX, y: bounds.minY + f.minY), anchor: .topLeading,
                proposal: ProposedViewSize(width: f.width, height: f.height))
        }
    }
}

private enum StripPadding {
    static let horizontal = StripLayout.sidePadding
}

// MARK: - Above the fader

/// A section's small heading ("INSERTS", "SENDS").
private struct StripHeading: View {
    let text: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        Text(theme.stripHeading(text))
            .font(theme.type.caption.font.weight(.semibold))
            .foregroundStyle(theme.text.secondary.color)
            .lineLimit(1)
            .accessibilityHidden(true)
    }
}

/// The colour bar, then the name (double-click renames; click selects,
/// ⌘-click adds; its menu), then the kind and Load All.
private struct StripHeader: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme
    @Environment(\.mixerPlacement) private var placement
    @FocusState private var renaming: Bool

    var body: some View {
        VStack(spacing: theme.spacing.xxs) {
            if appearancePreferences.colourBars {
                RoundedRectangle(cornerRadius: theme.radius.s)
                    .fill(StripPalette.bar(strip, theme).color)
                    .frame(height: 4)
                    .accessibilityHidden(true)
            }
            name
            Text(theme.stripLabel(strip.kindLabel))
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .lineLimit(1)
            let held = model.heldSlots(of: strip)
            if !held.isEmpty {
                Button("Load All") { model.load(held) }
                    .controlSize(.small)
                    .help("Safe mode kept these plugins off")
            }
        }
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder private var name: some View {
        if model.isRenaming(strip.id, in: placement) {
            TextField("Name", text: $model.renameDraft)
                .textFieldStyle(.roundedBorder)
                .font(theme.type.body.font)
                .focused($renaming)
                .onSubmit { model.commitRename() }
                .onExitCommand { model.cancelRename() }
                .onAppear { renaming = true }
        } else {
            Text(theme.stripLabel(strip.name))
                .font(theme.type.body.font.weight(.semibold))
                .foregroundStyle(theme.text.primary.color)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { model.beginRename(strip, in: placement) }
                .onTapGesture { model.select(strip.id, adding: NSEvent.modifierFlags.contains(.command)) }
                .help(strip.name)
                .contextMenu { menu }
        }
    }

    @ViewBuilder private var menu: some View {
        Button("Rename…") { model.beginRename(strip, in: placement) }
        Menu("Set All Strips to") {
            ForEach(AutomationModes.all, id: \.self) { m in
                Button(AutomationModes.title(m)) { model.setAllModes(m) }
            }
        }
        if strip.kind == .bus {
            Button("Remove Bus") { model.removeBus(strip.id) }
        }
        StripColourMenu(model: model, strip: strip)
        Divider()
        Button("MIDI Learn Maps…") { model.document.learn.showMaps(in: placement) }
        if !strip.freeOutputs.isEmpty {
            Menu("Add Outputs") {
                ForEach(strip.freeOutputs, id: \.self) { i in
                    Button("Output \(Int(i) + 1)") { model.addOutput(strip, i) }
                }
            }
        }
    }
}

/// "Colour ▸": None and the eight swatches, each in its colour (C-1,
/// Ruling 12); the lane header's menu makes the same call.
private struct StripColourMenu: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        Picker(
            "Colour",
            selection: Binding<SwatchRow?>(get: { strip.colour }, set: { model.setColour(strip, $0) })
        ) {
            Text(StripPalette.noColourTitle).tag(SwatchRow?.none)
            ForEach(StripPalette.swatches, id: \.self) { c in
                Label {
                    Text(StripPalette.title(c))
                } icon: {
                    // Its own colours, never a template (review M7).
                    Image(nsImage: SwatchImage.make(StripPalette.swatch(c, theme).nsColor))
                        .renderingMode(.original)
                }
                .tag(SwatchRow?.some(c))
            }
        }
        .pickerStyle(.menu)
    }
}

/// A strip menu's face: a small inset box with the text (and an optional
/// coloured dot) and a down mark.
private struct StripMenuFace: View {
    let text: String
    var dot: Color?
    var centred = false
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack(spacing: theme.spacing.xs) {
            if let dot {
                Circle().fill(dot).frame(width: 7, height: 7)
            }
            if centred { Spacer(minLength: 0) }
            Text(text)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.primary.color)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 0)
            AppIcon.menuDown.image
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
        }
        .padding(.horizontal, theme.spacing.xs)
        // The 24-point hit minimum (Ruling 4, review M2).
        .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumHit, maxHeight: ControlMetrics.minimumHit)
        .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(theme.surface.inset.color))
        .overlay(RoundedRectangle(cornerRadius: theme.radius.s).strokeBorder(theme.surface.separator.color))
        .contentShape(Rectangle())
    }
}

/// A menu drawn as a `StripMenuFace`.
private struct StripMenu<Content: View>: View {
    let text: String
    var dot: Color?
    var centred = false
    @ViewBuilder let content: () -> Content

    var body: some View {
        Menu {
            content()
        } label: {
            StripMenuFace(text: text, dot: dot, centred: centred)
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// A small button drawn as a dashed inset box ("EQ ＋", the inserts' "+").
private struct StripAddFace: View {
    let text: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        Text(text)
            .font(theme.type.caption.font.weight(.semibold))
            .foregroundStyle(theme.text.secondary.color)
            .lineLimit(1)
            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumHit)
            .background(
                RoundedRectangle(cornerRadius: theme.radius.s)
                    .strokeBorder(theme.surface.separator.color, style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
            .contentShape(Rectangle())
    }
}

/// The EQ slot (Ruling 14): empty, `EQ ＋` opens the effect browser for
/// the EQ; loaded, the plug-in's name and its bypass, a click opening its
/// window, and its menu.
struct EqSlotView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme
    @Environment(\.mixerPlacement) private var placement
    @Environment(\.openPluginWindow) private var openWindow

    var body: some View {
        Group {
            if let eq = strip.eq {
                loaded(eq)
            } else {
                Button {
                    model.openBrowser(.eq(strip: strip.id), in: placement)
                } label: {
                    StripAddFace(text: "EQ \u{FF0B}")
                }
                .buttonStyle(.plain)
                .help("Add an EQ")
                .accessibilityLabel("EQ of \(strip.name)")
                .accessibilityValue("empty")
            }
        }
        .popover(isPresented: browserShown) {
            PluginBrowserView(
                model: model.browserModel(.effect) {
                    PluginBrowserModel(kind: .effect) { appPlugins.browser(.effect) }
                }
            ) {
                model.pick($0)
            }
        }
    }

    private var browserShown: Binding<Bool> {
        let p = placement
        let target = MixerModel.BrowserTarget.eq(strip: strip.id)
        return Binding(
            get: { model.showsBrowser(in: p) && model.browser == target },
            set: { if !$0 { model.browser = nil } })
    }

    private func loaded(_ eq: SlotRow) -> some View {
        HStack(spacing: theme.spacing.xs) {
            Circle()
                .fill(SlotView.dot(eq, model: model, theme: theme))
                .frame(width: 6, height: 6)
            Text(eq.plugin.name)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.primary.color)
                .strikethrough(eq.bypass)
                .lineLimit(1)
            Spacer(minLength: 0)
            Button {
                model.toggleBypass(eq)
            } label: {
                AppIcon.eq.image
                    .font(theme.type.caption.font)
                    .foregroundStyle(eq.bypass ? theme.text.disabled.color : theme.state.play.color)
                    .frame(
                        width: ControlMetrics.hitSize(.toggle).width,
                        height: ControlMetrics.hitSize(.toggle).height)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(eq.bypass ? "Turn the EQ on" : "Bypass the EQ")
            .accessibilityLabel(eq.bypass ? "Turn On EQ of \(strip.name)" : "Bypass EQ of \(strip.name)")
        }
        .padding(.horizontal, theme.spacing.xs)
        .frame(maxWidth: .infinity, minHeight: 20)
        .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(theme.surface.inset.color))
        .contentShape(Rectangle())
        .onTapGesture { openWindow(eq.id) }
        .help("\(eq.plugin.name) (\(eq.plugin.vendor))")
        .accessibilityElement(children: .contain)
        .accessibilityLabel("EQ of \(strip.name)")
        .accessibilityValue(eq.plugin.name)
        .contextMenu { menu(eq) }
    }

    @ViewBuilder private func menu(_ eq: SlotRow) -> some View {
        Button("Open") { openWindow(eq.id) }
        if model.held.contains(eq.id) {
            Button("Load") { model.load([eq.id]) }
        }
        Button("Replace…") { model.openBrowser(.eq(strip: strip.id), in: placement) }
        Button(eq.bypass ? "Turn On" : "Bypass") { model.toggleBypass(eq) }
        if eq.hasSidechainInput {
            Menu("Sidechain") {
                Button("None") { model.setSidechain(eq.id, source: nil) }
                ForEach(model.sidechainSources(for: strip), id: \.id) { s in
                    Button((eq.sidechain == s.id ? "✓ " : "") + s.name) {
                        model.setSidechain(eq.id, source: s.id)
                    }
                }
            }
        }
        Divider()
        Button("Remove EQ") { model.clearEq(strip) }
    }
}

/// The inserts: today's slot rows (reorder, drop, sidechain, Load,
/// Remove), then "+".
private struct StripInserts: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @Environment(\.mixerPlacement) private var placement
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            StripHeading(text: "Inserts")
            ForEach(Array(strip.inserts.enumerated()), id: \.element.id) { i, slot in
                SlotView(model: model, strip: strip, slot: slot, index: i)
            }
            Button {
                model.openBrowser(.insert(strip: strip.id), in: placement)
            } label: {
                StripAddFace(text: "+")
            }
            .buttonStyle(.plain)
            .help("Add an effect")
            .accessibilityLabel("Add an effect to \(strip.name)")
            .popover(
                isPresented: Binding(
                    get: { model.showsBrowser(in: placement) && model.browser == .insert(strip: strip.id) },
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
                guard let id = items.first.flatMap(SlotView.slotID) else { return false }
                model.moveInsert(id, to: strip.id, at: strip.inserts.count)
                return true
            }
        }
    }
}

/// The sends (Ruling 14): per send, its target's name on its own line,
/// then a small knob beside a number box with the level; then the "Send"
/// menu to add one.
struct SendKnobRow: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let targets = model.targets(for: strip)
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            StripHeading(text: "Sends")
            ForEach(Array(strip.sends.enumerated()), id: \.offset) { i, send in
                SendKnob(model: model, strip: strip, index: i, send: send)
            }
            if !targets.isEmpty {
                StripMenu(text: "Send") {
                    ForEach(targets, id: \.id) { b in
                        Button(b.name) { model.addSend(strip.id, to: b.id) }
                    }
                }
                .help("Add a send")
                .accessibilityLabel("Add a send to \(strip.name)")
            }
        }
    }
}

/// One send: the name (struck through when off) and a PRE chip, then the
/// knob and its level box; its menu holds Pre-Fader, On, MIDI Learn and
/// Remove Send.
private struct SendKnob: View {
    let model: MixerModel
    let strip: StripRow
    let index: Int
    let send: SendRow
    @Environment(\.symbioticTheme) private var theme

    private var target: AutomationTargetRow { .sendLevel(strip: strip.id, send: MixerModel.sendIndex(index)) }
    private var db: Double { model.sendLevelDb(strip, index) }

    var body: some View {
        let name = model.stripName(send.target)
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            HStack(spacing: theme.spacing.xs) {
                Text(name)
                    .font(theme.type.caption.font)
                    .foregroundStyle(send.enabled ? theme.text.primary.color : theme.text.disabled.color)
                    .strikethrough(!send.enabled)
                    .lineLimit(1)
                Spacer(minLength: 0)
                if send.preFader {
                    Text("PRE")
                        .font(theme.type.caption.font.weight(.bold))
                        .foregroundStyle(Color(nsColor: theme.legend(on: theme.control.send.nsColor)))
                        .padding(.horizontal, theme.spacing.xs)
                        .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(theme.control.send.color))
                        .help("Pre-fader")
                }
            }
            HStack(spacing: theme.spacing.xs) {
                Knob(
                    value: MixerMath.faderPosition(db), origin: 0,
                    arcColour: StripPalette.knob(.send, theme).color,
                    style: theme.stripKnob(.small), palette: KnobPalette(theme: theme), relief: Relief(theme: theme),
                    accessibilityLabel: "Send knob \(index + 1) of \(strip.name)",
                    accessibilityValue: StripEntry.levelText(db),
                    onBegin: { model.touch(target) },
                    onChange: { p in model.move(target, to: AutomationLaw.faderNorm(MixerMath.faderDb(p))) },
                    onEnd: { model.release(target) },
                    onReset: { model.click(target, AutomationLaw.faderNorm(0)) })
                    .frame(
                        width: ControlMetrics.hitSize(.knob(.small)).width,
                        height: ControlMetrics.hitSize(.knob(.small)).height)
                    .opacity(send.enabled ? 1 : 0.45)
                    .touchRing(model.isTouched(target))
                NumberBox(
                    text: StripEntry.levelText(db), accessibilityLabel: "Send \(index + 1) of \(strip.name)",
                    palette: theme.readoutBox,
                    commit: { model.typeSend(strip, index, $0) },
                    onRefuse: { model.document.editor.status = StripEntry.levelHint })
                    .frame(
                        width: ControlMetrics.hitSize(.numberBox).width,
                        height: ControlMetrics.hitSize(.numberBox).height)
                    .readoutEdge(theme)
                    .accessibilityIdentifier(ControlUI.numberBoxID)
            }
        }
        .contextMenu {
            Toggle("Pre-Fader", isOn: Binding(get: { send.preFader }, set: { v in model.editSend(strip.id, index) { $0.preFader = v } }))
            Toggle("On", isOn: Binding(get: { send.enabled }, set: { v in model.editSend(strip.id, index) { $0.enabled = v } }))
            LearnItem(model: model, target: target)
            Divider()
            Button("Remove Send") { model.removeSend(strip.id, index) }
        }
        .help("Send to \(name): \(MixerMath.dbLabel(db)) dB")
    }
}

/// "→ ‹Master or bus›": Master and the buses.
private struct StripOutputMenu: View {
    let model: MixerModel
    let strip: StripRow

    var body: some View {
        StripMenu(text: "\u{2192} \(model.outputName(strip))") {
            Button("Master") { model.setOutput(strip.id, bus: nil) }
            ForEach(model.targets(for: strip), id: \.id) { b in
                Button(b.name) { model.setOutput(strip.id, bus: b.id) }
            }
        }
        .help("Output of \(strip.name)")
        .accessibilityLabel("Output of \(strip.name)")
    }
}

/// The pan: a medium knob, bipolar from the centre, and its typed box.
private struct StripPan: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    private var target: AutomationTargetRow { .stripPan(strip: strip.id) }

    var body: some View {
        let pan = model.pan(strip)
        HStack(spacing: theme.spacing.s) {
            Knob(
                value: (pan + 1) / 2, origin: 0.5, arcColour: StripPalette.knob(.pan, theme).color,
                style: theme.stripKnob(.medium), palette: KnobPalette(theme: theme), relief: Relief(theme: theme),
                accessibilityLabel: "Pan knob of \(strip.name)", accessibilityValue: StripEntry.panText(pan),
                onBegin: { model.touch(target) },
                onChange: { model.move(target, to: $0) },
                onEnd: { model.release(target) },
                onReset: { model.click(target, 0.5) })
                .frame(
                    width: ControlMetrics.hitSize(.knob(.medium)).width,
                    height: ControlMetrics.hitSize(.knob(.medium)).height)
                .touchRing(model.isTouched(target))
                .contextMenu { LearnItem(model: model, target: target) }
                .help("Pan: double-click for the centre")
            NumberBox(
                text: StripEntry.panText(pan), accessibilityLabel: "Pan of \(strip.name)",
                palette: theme.readoutBox,
                commit: { model.typePan(strip, $0) },
                onRefuse: { model.document.editor.status = StripEntry.panHint })
                .frame(
                    width: ControlMetrics.hitSize(.numberBox).width,
                    height: ControlMetrics.hitSize(.numberBox).height)
                .readoutEdge(theme)
                .accessibilityIdentifier(ControlUI.numberBoxID)
            Spacer(minLength: 0)
        }
    }
}

// MARK: - The fader and below

/// The dB scale, the long-throw fader (its cap in the strip kind's colour,
/// never the strip's own) and the meter. It is given `throw + faderCap`
/// points by the column and turns them back into the throw.
struct StripFaderSection: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    private var target: AutomationTargetRow { .stripFader(strip: strip.id) }

    var body: some View {
        GeometryReader { g in
            let throwLength = max(g.size.height - StripLayout.faderCap, 0)
            let db = model.gainDb(strip)
            HStack(alignment: .top, spacing: theme.spacing.xs) {
                Spacer(minLength: 0)
                Fader(
                    value: MixerMath.faderPosition(db), throwLength: throwLength,
                    cap: StripPalette.cap(strip.kindColour, theme).color, palette: FaderPalette(theme: theme),
                    relief: Relief(theme: theme),
                    marks: StripLayout.faderMarks.map { FaderMark(position: $0.position, label: $0.label) },
                    accessibilityLabel: "Fader of \(strip.name)", accessibilityValue: StripEntry.levelText(db),
                    onBegin: { model.touch(target) },
                    onChange: { model.move(target, to: AutomationLaw.faderNorm(MixerMath.faderDb($0))) },
                    onEnd: { model.release(target) },
                    onReset: { model.click(target, AutomationLaw.faderNorm(0)) })
                    .touchRing(model.isTouched(target))
                    .contextMenu { LearnItem(model: model, target: target) }
                    .help("Double-click: 0 dB")
                // Level with the fader's well, which spans the cap's travel:
                // from half the drawn cap (less 2) for `throw + 4` points.
                StripMeter(level: model.meters[strip.id] ?? .silent)
                    .frame(width: 14, height: throwLength + 4)
                    .padding(.top, ControlMetrics.faderCap.height / 2 - 2)
                Spacer(minLength: 0)
            }
        }
    }
}

/// The peak meter, left and right, in the theme's meter bands.
private struct StripMeter: View {
    let level: MeterLevel
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack(spacing: theme.spacing.xxs) {
            bar(level.left)
            bar(level.right)
        }
        .accessibilityHidden(true)
    }

    private func bar(_ peak: Float) -> some View {
        GeometryReader { g in
            ZStack(alignment: .bottom) {
                Rectangle().fill(theme.meter.background.color)
                Rectangle()
                    .fill(StripPalette.meter(peak, theme).color)
                    .frame(height: g.size.height * MixerMath.meterFill(peak))
            }
        }
    }
}

/// The level, typed (`−4.2 dB`, `0.0 dB`, `−∞ dB`).
private struct StripLevelBox: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack {
            Spacer(minLength: 0)
            NumberBox(
                text: StripEntry.levelText(model.gainDb(strip)), accessibilityLabel: "Level of \(strip.name)",
                palette: theme.readoutBox,
                commit: { model.typeLevel(strip, $0) },
                onRefuse: { model.document.editor.status = StripEntry.levelHint })
                .frame(
                    width: ControlMetrics.hitSize(.numberBox).width,
                    height: ControlMetrics.hitSize(.numberBox).height)
                .readoutEdge(theme)
                .accessibilityIdentifier(ControlUI.numberBoxID)
                .help("Type a level in dB")
            Spacer(minLength: 0)
        }
    }
}

/// M S R I (Ruling 14): mute and solo (master has no solo), record arm and
/// input monitor of the strip's input lane, disabled without one.
struct StripButtons: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let lane = model.inputLane(for: strip)
        let armable = model.canArm(strip)
        HStack(spacing: theme.spacing.xs) {
            StripToggle(letter: "M", on: strip.mute, colour: theme.state.mute, label: "Mute \(strip.name)") {
                model.toggleMute(strip)
            }
            .touchRing(model.isTouched(.stripMute(strip: strip.id)))
            .contextMenu { LearnItem(model: model, target: .stripMute(strip: strip.id)) }
            if strip.kind != .master {
                StripToggle(letter: "S", on: strip.solo, colour: theme.state.solo, label: "Solo \(strip.name)") {
                    model.toggleSolo(strip)
                }
                .touchRing(model.isTouched(.stripSolo(strip: strip.id)))
                .contextMenu { LearnItem(model: model, target: .stripSolo(strip: strip.id)) }
            } else {
                Color.clear.frame(width: ControlMetrics.hitSize(.toggle).width)
            }
            StripToggle(letter: "R", on: lane?.arm == true, colour: theme.state.record, label: "Record arm \(strip.name)") {
                model.toggleArm(strip)
            }
            .disabled(!armable)
            .help(armable ? "Record arm" : "Arm an audio lane in its header")
            StripToggle(
                letter: "I", on: (lane?.monitor ?? .off) != .off,
                colour: StripPalette.monitor(lane?.monitor ?? .off, theme),
                label: "Input monitor \(strip.name)"
            ) {
                model.cycleMonitor(strip)
            }
            .disabled(!armable)
            .help(lane.map { LaneInputs.help($0.monitor) } ?? "Input monitoring is for audio lanes")
        }
        .frame(maxWidth: .infinity)
        .help(strip.soloMuted ? "Silent: other strips are soloed" : "Mute / Solo")
    }
}

/// One of M S R I: 24 by 24, lit in its state's colour.
private struct StripToggle: View {
    let letter: String
    let on: Bool
    let colour: ThemeColor
    let label: String
    let action: () -> Void
    @Environment(\.symbioticTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let size = ControlMetrics.hitSize(.toggle)
        let fill = on ? colour.nsColor : theme.surface.inset.nsColor
        Button(action: action) {
            Text(letter)
                .font(theme.type.label.font.weight(.bold))
                .foregroundStyle(on ? Color(nsColor: theme.legend(on: fill)) : theme.text.secondary.color)
                .frame(width: size.width, height: size.height)
                .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(Color(nsColor: fill)))
                .overlay(RoundedRectangle(cornerRadius: theme.radius.s).strokeBorder(theme.surface.separator.color))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .opacity(isEnabled ? 1 : 0.45)
        .accessibilityLabel(label)
        .accessibilityValue(on ? "on" : "off")
    }
}

/// The automation mode menu over the six modes, tinted by the mode.
private struct StripModeMenu: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        StripMenu(
            text: AutomationModes.title(strip.automation),
            dot: StripPalette.automation(strip.automation, theme).color
        ) {
            ForEach(AutomationModes.all, id: \.self) { m in
                Button {
                    model.setMode(strip.id, m)
                } label: {
                    if m == strip.automation {
                        AppIcon.confirm.label(AutomationModes.title(m))
                    } else {
                        Text(AutomationModes.title(m))
                    }
                }
            }
        }
        .help("Automation: \(AutomationModes.title(strip.automation))")
        .accessibilityLabel("Automation of \(strip.name)")
    }
}

// MARK: - Theme styling

extension Theme {
    /// The strip's knob style: the family's skirt-band arc, except where
    /// the theme's skirt is knurled (Console), whose knobs show the value
    /// by their pointer on a cap in the function's colour (arc off).
    func stripKnob(_ size: KnobSize) -> KnobStyle {
        style.knobSkirt == .knurled ? KnobStyle(size: size, arc: .off, cap: .accent) : KnobStyle(size: size)
    }

    /// A name or kind in the theme's label case (Console's capitals,
    /// Flat's lower case).
    func stripLabel(_ text: String) -> String {
        switch style.labels {
        case .plain: text
        case .lowercase: text.lowercased()
        case .caps, .condensedCaps: text.uppercased()
        }
    }

    /// The number boxes' palette: the readout look (fill, digits) from
    /// `StripPalette.numberBox`, the rest as `NumberBoxPalette(theme:)`.
    var readoutBox: NumberBoxPalette {
        let base = NumberBoxPalette(theme: self)
        let r = StripPalette.numberBox(self)
        return NumberBoxPalette(
            background: r.fill.color, editingBackground: r.fill.color, text: r.digits.color,
            refused: base.refused, focus: base.focus, cornerRadius: base.cornerRadius, font: base.font)
    }

    /// A section heading: capitals, except in a lower-case theme.
    func stripHeading(_ text: String) -> String {
        style.labels == .lowercase ? text.lowercased() : text.uppercased()
    }
}

/// A band's shade.
enum StripBandShade { case dark, light }

/// A section's band behind it, the strip's full width, in a banded theme
/// (Console's dark and light panels); nothing elsewhere.
private struct StripBand: ViewModifier {
    let shade: StripBandShade?
    @Environment(\.symbioticTheme) private var theme

    func body(content: Content) -> some View {
        if let shade, theme.style.panelBands == .banded {
            content
                .padding(.horizontal, StripPadding.horizontal)
                .padding(.vertical, theme.spacing.xs)
                .background((shade == .light ? theme.band.light : theme.band.dark).color)
                .padding(.horizontal, -StripPadding.horizontal)
        } else {
            content
        }
    }
}

extension View {
    /// The readout's edge around a number box (`StripPalette.numberBox`).
    fileprivate func readoutEdge(_ t: Theme) -> some View {
        overlay(
            RoundedRectangle(cornerRadius: t.radius.s)
                .strokeBorder(StripPalette.numberBox(t).edge.color.opacity(0.35))
                .frame(width: ControlMetrics.numberBox.width, height: ControlMetrics.numberBox.height)
                .allowsHitTesting(false))
    }

    fileprivate func stripBand(_ shade: StripBandShade?) -> some View { modifier(StripBand(shade: shade)) }
}
