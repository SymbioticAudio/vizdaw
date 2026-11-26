import AppKit
import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// A VCA's cells in the strips' grid (groups-vcas-transport Task 9, spec
/// §2): the colour bar and name (its menu: Rename VCA…, Delete VCA), the
/// member count, the fader, the level box, M and S, and the automation
/// mode menu over the VCA's four modes. It carries no audio, so it has no
/// meter and the rows above the fader it has no part in stay empty.
struct VcaStripCells: View {
    @Bindable var model: MixerModel
    let vca: VcaRow
    let column: Int

    var body: some View {
        VcaStripHeader(model: model, vca: vca)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .cell(column, .row(.header))
        VcaStripFader(model: model, vca: vca).cell(column, .fader)
        VcaStripLevel(model: model, vca: vca).cell(column, .level)
        VcaStripButtons(model: model, vca: vca).cell(column, .buttons)
        VcaStripModeMenu(model: model, vca: vca).cell(column, .mode)
    }
}

/// The colour bar in the VCA's colour, its name (double-click renames; its
/// menu) and "VCA · n members".
private struct VcaStripHeader: View {
    @Bindable var model: MixerModel
    let vca: VcaRow
    @Environment(\.symbioticTheme) private var theme
    @Environment(\.mixerPlacement) private var placement

    var body: some View {
        VStack(spacing: theme.spacing.xxs) {
            if appearancePreferences.colourBars {
                RoundedRectangle(cornerRadius: theme.radius.s)
                    .fill(StripPalette.swatch(vca.colour, theme).color)
                    .frame(height: StripLayout.colourBarHeight)
                    .accessibilityHidden(true)
            }
            Text(theme.stripLabel(vca.name))
                .font(theme.stripLabelFont(theme.type.body, weight: .semibold))
                .foregroundStyle(theme.text.primary.color)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { model.beginVcaRename(vca, in: placement) }
                .help(vca.name)
                .accessibilityLabel(VcaStripLabels.title(vca.name))
                .accessibilityAddTraits(.isHeader)
                .popover(isPresented: renaming, arrowEdge: .bottom) { VcaRenameField(model: model) }
                .contextMenu { VcaMenuItems(model: model, vca: vca) }
            Text(theme.stripLabel("VCA · " + VcaStripLabels.members(vca.members.count)))
                .font(theme.stripLabelFont(theme.type.caption))
                .foregroundStyle(theme.text.secondary.color)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity)
    }

    private var renaming: Binding<Bool> {
        let p = placement
        let id = vca.id
        return Binding(
            get: { model.isRenamingVca(id, in: p) },
            set: { if !$0 && model.isRenamingVca(id, in: p) { model.cancelVcaRename() } })
    }
}

/// The rename popover's field: Return renames, Escape cancels.
private struct VcaRenameField: View {
    @Bindable var model: MixerModel
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        TextField("Name", text: $model.renameDraft)
            .textFieldStyle(.roundedBorder)
            .font(theme.type.body.font)
            .frame(width: 180)
            .padding(theme.spacing.s)
            .onSubmit { model.commitVcaRename() }
            .onExitCommand { model.cancelVcaRename() }
            .accessibilityLabel("Name of the VCA")
    }
}

/// The VCA's menu items: Rename VCA… (a user click opens its field) and
/// Delete VCA (its members keep their faders, spec §2).
struct VcaMenuItems: View {
    let model: MixerModel
    let vca: VcaRow
    @Environment(\.mixerPlacement) private var placement

    var body: some View {
        Button("Rename VCA…") { model.beginVcaRename(vca, in: placement) }
        Button("Delete VCA") { model.deleteVca(vca.id) }
    }
}

/// The VCA's fader, its cap in the VCA's colour, with the strips' scale; a
/// drag is a gesture on its automation target.
private struct VcaStripFader: View {
    let model: MixerModel
    let vca: VcaRow
    @Environment(\.symbioticTheme) private var theme

    private var target: AutomationTargetRow { .vcaFader(vca: vca.id) }

    var body: some View {
        GeometryReader { g in
            let throwLength = max(g.size.height - StripLayout.faderCap, 0)
            let db = model.vcaGainDb(vca)
            HStack(alignment: .top, spacing: theme.spacing.xs) {
                Spacer(minLength: 0)
                Fader(
                    value: MixerMath.faderPosition(db), throwLength: throwLength,
                    cap: StripPalette.swatch(vca.colour, theme).color, palette: FaderPalette(theme: theme),
                    relief: Relief(theme: theme),
                    marks: StripLayout.faderMarks.map { FaderMark(position: $0.position, label: $0.label) },
                    accessibilityLabel: VcaStripLabels.fader(vca.name), accessibilityValue: StripEntry.levelText(db),
                    onBegin: { model.touch(target) },
                    onChange: { model.move(target, to: AutomationLaw.faderNorm(MixerMath.faderDb($0))) },
                    onEnd: { model.release(target) },
                    onReset: { model.click(target, AutomationLaw.faderNorm(0)) })
                    .touchRing(model.isTouched(target))
                    .help("Double-click: 0 dB")
                Spacer(minLength: 0)
            }
        }
    }
}

/// The VCA's level, typed.
private struct VcaStripLevel: View {
    let model: MixerModel
    let vca: VcaRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack {
            Spacer(minLength: 0)
            NumberBox(
                text: StripEntry.levelText(model.vcaGainDb(vca)), accessibilityLabel: VcaStripLabels.level(vca.name),
                palette: theme.readoutBox,
                commit: { model.typeVcaLevel(vca, $0) },
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

/// M and S: the VCA's own flags, never its members' (spec §2a).
private struct VcaStripButtons: View {
    let model: MixerModel
    let vca: VcaRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack(spacing: theme.spacing.xs) {
            StripToggle(letter: "M", on: vca.mute, colour: theme.state.mute, label: VcaStripLabels.mute(vca.name)) {
                model.toggleVcaMute(vca)
            }
            StripToggle(letter: "S", on: vca.solo, colour: theme.state.solo, label: VcaStripLabels.solo(vca.name)) {
                model.toggleVcaSolo(vca)
            }
        }
        .frame(maxWidth: .infinity)
        .help("Mute / Solo the VCA's members")
    }
}

/// The automation mode menu over Off, Read, Touch and Latch (Ruling 6).
private struct VcaStripModeMenu: View {
    let model: MixerModel
    let vca: VcaRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        StripMenu(
            text: AutomationModes.title(vca.automation),
            dot: StripPalette.automation(vca.automation, theme).color
        ) {
            ForEach(AutomationModes.vca, id: \.self) { m in
                Button {
                    model.setVcaMode(vca, m)
                } label: {
                    if m == vca.automation {
                        AppIcon.confirm.label(AutomationModes.title(m))
                    } else {
                        Text(AutomationModes.title(m))
                    }
                }
            }
        }
        .help("Automation: \(AutomationModes.title(vca.automation))")
        .accessibilityLabel(VcaStripLabels.mode)
    }
}

/// A member strip's VCA offset row, under its level box: "VCA −3.0" while
/// its VCAs change what is heard, and the "VCA" mark, lit while a VCA
/// mutes or solos it (spec §2, §2a). Empty for a strip in no VCA.
struct StripVcaOffset: View {
    let model: MixerModel
    let strip: StripRow
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        HStack(spacing: theme.spacing.xs) {
            Spacer(minLength: 0)
            if let text = MixerModel.vcaOffsetText(model.vcaOffsetDb(strip)) {
                Text(text)
                    .font(theme.type.caption.font.monospacedDigit())
                    .foregroundStyle(theme.text.secondary.color)
                    .lineLimit(1)
                    .help("Its VCAs change what is heard by this much")
                    .accessibilityLabel(VcaStripLabels.offset(text))
            }
            if !strip.vcas.isEmpty {
                let lit = model.vcaMark(strip)
                Text("VCA")
                    .font(theme.type.caption.font.weight(.bold))
                    .foregroundStyle(
                        lit ? Color(nsColor: theme.legend(on: theme.state.mute.nsColor)) : theme.text.disabled.color)
                    .padding(.horizontal, theme.spacing.xxs)
                    .background(
                        RoundedRectangle(cornerRadius: theme.radius.s)
                            .fill(lit ? theme.state.mute.color : theme.surface.inset.color))
                    .help(lit ? "A VCA mutes or solos this strip" : "In a VCA")
                    .accessibilityLabel(VcaStripLabels.mark(strip.name))
                    .accessibilityValue(lit ? "on" : "off")
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
