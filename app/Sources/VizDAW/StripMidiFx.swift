import AppKit
import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The channel strip's MIDI FX section beneath Inserts (plan 5g-1 Task 9,
/// Ruling 16): the input effect chain of the track the strip edits
/// (`MixerModel.midiFxLane(for:in:)`), one slot per effect, then "+" while
/// the chain holds fewer than 8. The heading lights while the chain has an
/// effect that is on. A click on "+" pops up the effect menu, a click on a
/// slot its settings; with the control channel on neither opens (the
/// model sets the status line instead).
struct StripMidiFx: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @Environment(\.mixerPlacement) private var placement
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let lane = model.midiFxLane(for: strip, in: placement)
        let fx = model.document.inputFx
        let lit = lane.map(InputFxModel.isLit) ?? false
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            StripHeading(text: "MIDI FX", lit: lit)
            if let lane {
                let rows = fx.chain(of: lane)
                ForEach(Array(rows.enumerated()), id: \.offset) { i, row in
                    MidiFxSlot(fx: fx, lane: lane, strip: strip, index: i, row: row)
                }
                if rows.count < 8 {
                    Button {
                        if fx.clickEmpty(lane: lane.id) { MidiFxMenu.popUp(fx) }
                    } label: {
                        StripAddFace(text: "+")
                    }
                    .buttonStyle(.plain)
                    .help("Add a MIDI effect")
                    .accessibilityLabel("Add a MIDI effect to \(lane.name)")
                }
            } else {
                Button {
                } label: {
                    StripAddFace(text: "+")
                }
                .buttonStyle(.plain)
                .disabled(true)
                .help("No track in this sequence plays through this strip.")
                .accessibilityLabel("Add a MIDI effect")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("MIDI FX of \(lane?.name ?? strip.name)")
        .accessibilityValue(lit ? "on" : "off")
    }
}

/// One MIDI FX slot, drawn as an insert slot (`SlotView`): the bypass dot,
/// the effect's title (struck through when bypassed) and its settings in
/// one line. A click shows its settings popover; a drag reorders it within
/// its track's chain. No context menu: a right-click from the control
/// channel would open it. Bypass is on the dot, Remove in the popover.
struct MidiFxSlot: View {
    let fx: InputFxModel
    let lane: LaneRow
    let strip: StripRow
    let index: Int
    let row: InputFxRow
    @Environment(\.mixerPlacement) private var placement
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let i = index
        let bypass = row.bypass
        let title = InputFxModel.title(row.kind)
        let summary = InputFxModel.summary(row.kind)
        HStack(spacing: theme.spacing.xs) {
            Button {
                fx.toggleBypass(i, lane: lane.id)
            } label: {
                Circle()
                    .fill(bypass ? theme.text.disabled.color : theme.state.play.color)
                    .frame(width: 6, height: 6)
                    .frame(
                        width: ControlMetrics.hitSize(.toggle).width,
                        height: ControlMetrics.hitSize(.toggle).height)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(bypass ? "Turn \(title) on" : "Bypass \(title)")
            Text(title)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.primary.color)
                .strikethrough(bypass)
                .lineLimit(1)
            Text(summary)
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, theme.spacing.xs)
        .frame(maxWidth: .infinity, minHeight: 20)
        .background(RoundedRectangle(cornerRadius: theme.radius.s).fill(theme.surface.inset.color))
        .overlay(RoundedRectangle(cornerRadius: theme.radius.s).strokeBorder(theme.surface.separator.color))
        .contentShape(Rectangle())
        .help("\(title), \(summary)")
        .onTapGesture { fx.clickSlot(i, lane: lane.id, strip: strip.id, in: placement) }
        .popover(
            isPresented: Binding(
                get: { fx.showsSettings(strip: strip.id, index: i, in: placement) },
                set: { shown in
                    // Only this slot's popover closing clears the settings:
                    // a click on another slot has already moved them there.
                    if !shown, fx.showsSettings(strip: strip.id, index: i, in: placement) { fx.closeSettings() }
                })
        ) {
            InputFxSettings(model: fx)
        }
        .draggable(InputFxModel.dragText(lane: lane.id, index: i))
        .dropDestination(for: String.self) { items, _ in
            items.first.map { fx.drop($0, onto: i, lane: lane.id) } ?? false
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title), \(summary)")
        .accessibilityValue(bypass ? "bypassed" : "on")
        .accessibilityAddTraits(.isButton)
        .accessibilityHint("Opens its settings")
        .accessibilityAction(named: bypass ? "Turn On" : "Bypass") { fx.toggleBypass(i, lane: lane.id) }
    }
}
