import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The left pane's MIDI strip (plan 5g-1b, spec §6.8a): one column of the
/// strips' grid for the focused MIDI track, scrolling when the pane is too
/// short (as `ChannelStrip`). `dest` is its external destination, nil for
/// none.
struct MidiChannelStrip: View {
    @Bindable var model: MixerModel
    let lane: LaneRow
    let dest: DestRow?
    let height: CGFloat

    var body: some View {
        ScrollView(.vertical) {
            ChannelStrips(
                model: model, stripColumns: [.track(lane, dest: dest)], dividerBefore: nil, height: height)
        }
    }
}

/// A MIDI strip's cells in the strips' grid (§6.8a), top to bottom: the
/// name (the left pane's `menu`: the track's destination menu, Ruling 8),
/// MIDI FX on `lane`, pan, the 0–127 fader with its note-activity meter,
/// the typeable volume, and M and R. The rows it has no part in stay
/// empty, as a strip without sends. Unset values are dimmed (Ruling 2);
/// with no destination pan, the fader and M are off while MIDI FX and R
/// work. `lane` is the track R and MIDI FX act on (Ruling 9).
struct MidiStripCells: View {
    @Bindable var model: MixerModel
    let dest: DestRow?
    let lane: LaneRow?
    let menu: Bool
    let column: Int

    private var name: String { dest?.name ?? MidiStripMath.noDestination }

    var body: some View {
        row(.header) { MidiStripHeader(model: model, dest: dest, lane: lane, menu: menu) }
        row(.midiFx, band: .dark) {
            StripMidiFx(
                model: model, lane: lane, key: MixerModel.midiFxKey(dest: dest?.id, lane: lane?.id ?? 0),
                name: name, emptyHelp: "No track in this sequence plays this destination.")
        }
        row(.pan, band: .dark) { MidiStripPan(model: model, dest: dest?.id, name: name) }
        MidiStripFader(model: model, dest: dest?.id, name: name).cell(column, .fader)
        MidiStripLevel(model: model, dest: dest?.id, name: name).cell(column, .level)
        MidiStripButtons(model: model, dest: dest?.id, lane: lane, name: name).cell(column, .buttons)
    }

    /// A row above the fader, as `StripCells.row`.
    private func row(_ kind: StripRowKind, band: StripBandShade? = nil, @ViewBuilder _ content: () -> some View)
        -> some View
    {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .stripBand(band)
            .cell(column, .row(kind))
    }
}

/// The colour bar, the destination's name (the left pane: its menu), and
/// "MIDI · Ch n".
private struct MidiStripHeader: View {
    @Bindable var model: MixerModel
    let dest: DestRow?
    let lane: LaneRow?
    let menu: Bool
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let name = dest?.name ?? MidiStripMath.noDestination
        VStack(spacing: theme.spacing.xxs) {
            if appearancePreferences.colourBars {
                RoundedRectangle(cornerRadius: theme.radius.s)
                    .fill(StripPalette.cap(.instrument, theme).color)
                    .frame(height: StripLayout.colourBarHeight)
                    .accessibilityHidden(true)
            }
            if menu, let lane {
                StripMenu(text: theme.stripLabel(name), centred: true) {
                    destinationItem(lane, nil, MidiStripMath.noDestination)
                    Divider()
                    ForEach(model.destinations, id: \.id) { d in
                        destinationItem(lane, d.id, d.name)
                    }
                }
                .help("Destination of \(lane.name)")
                .accessibilityLabel("Destination of \(lane.name)")
                .accessibilityValue(name)
            } else {
                Text(theme.stripLabel(name))
                    .font(theme.type.body.font.weight(.semibold))
                    .foregroundStyle(dest == nil ? theme.text.secondary.color : theme.text.primary.color)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: .infinity)
                    .help(name)
            }
            Text(theme.stripLabel(MidiStripMath.kindLabel(channel: dest?.channel)))
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity)
    }

    private func destinationItem(_ lane: LaneRow, _ id: UInt64?, _ title: String) -> some View {
        Button {
            model.setMidiDest(lane, id)
        } label: {
            if lane.midiDest == id {
                AppIcon.confirm.label(title)
            } else {
                Text(title)
            }
        }
    }
}

/// CC 10: a medium knob, bipolar from the centre, and its typed box
/// ("L 64" … "C" … "R 63"). A drag previews and commits on release.
private struct MidiStripPan: View {
    let model: MixerModel
    let dest: UInt64?
    let name: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let pan = dest.flatMap { model.midiMix($0) }?.pan
        let label = MidiStripMath.panLabel(pan)
        HStack(spacing: theme.spacing.s) {
            Knob(
                value: MidiStripMath.panPosition(pan), origin: MidiStripMath.panPosition(MidiStripMath.panCentre),
                arcColour: StripPalette.knob(.pan, theme).color,
                style: theme.stripKnob(.medium), palette: KnobPalette(theme: theme), relief: Relief(theme: theme),
                accessibilityLabel: "Pan knob of \(name)", accessibilityValue: label, step: 1.0 / 127,
                onBegin: {},
                onChange: { p in
                    guard let dest else { return }
                    model.midiDrag(
                        dest, volume: nil, pan: MidiStripMath.value(at: p), now: ProcessInfo.processInfo.systemUptime)
                },
                onEnd: { if let dest { model.midiRelease(dest) } },
                onReset: { if let dest { model.setMidiPan(dest, MidiStripMath.panCentre) } })
                .frame(
                    width: ControlMetrics.hitSize(.knob(.medium)).width,
                    height: ControlMetrics.hitSize(.knob(.medium)).height)
                .opacity(pan == nil ? MidiStripMath.dimmed : 1)
                .help("Pan: double-click for the centre")
            NumberBox(
                text: label, accessibilityLabel: "Pan of \(name)", palette: theme.readoutBox,
                commit: { t in dest.map { model.typeMidiPan($0, t) } ?? false },
                onRefuse: { model.document.editor.status = MidiStripMath.panHint })
                .frame(
                    width: ControlMetrics.hitSize(.numberBox).width,
                    height: ControlMetrics.hitSize(.numberBox).height)
                .readoutEdge(theme)
                .opacity(pan == nil ? MidiStripMath.dimmed : 1)
                .accessibilityIdentifier(ControlUI.numberBoxID(named: "midi-pan"))
            Spacer(minLength: 0)
        }
        .disabled(dest == nil)
        .onDisappear { if let dest { model.midiCancel(dest) } }
    }
}

/// CC 7: the 0–127 scale (127, 100, 64, 32, 0), the long-throw fader,
/// linear in the value (Ruling 5), and the note-activity meter. A drag
/// previews and commits on release; a double-click sets 100.
private struct MidiStripFader: View {
    let model: MixerModel
    let dest: UInt64?
    let name: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        GeometryReader { g in
            let throwLength = max(g.size.height - StripLayout.faderCap, 0)
            let volume = dest.flatMap { model.midiMix($0) }?.volume
            HStack(alignment: .top, spacing: theme.spacing.xs) {
                Spacer(minLength: 0)
                Fader(
                    value: MidiStripMath.position(volume), throwLength: throwLength,
                    cap: StripPalette.cap(.instrument, theme).color, palette: FaderPalette(theme: theme),
                    relief: Relief(theme: theme),
                    marks: MidiStripMath.faderMarks.map { FaderMark(position: $0.position, label: $0.label) },
                    accessibilityLabel: "Fader of \(name)", accessibilityValue: MidiStripMath.volumeLabel(volume),
                    step: 1.0 / 127,
                    onBegin: {},
                    onChange: { p in
                        guard let dest else { return }
                        model.midiDrag(
                            dest, volume: MidiStripMath.value(at: p), pan: nil,
                            now: ProcessInfo.processInfo.systemUptime)
                    },
                    onEnd: { if let dest { model.midiRelease(dest) } },
                    onReset: { if let dest { model.setMidiVolume(dest, MidiStripMath.defaultVolume) } })
                    .opacity(volume == nil ? MidiStripMath.dimmed : 1)
                    .help("Double-click: 100")
                MidiActivityMeter(activity: dest.flatMap { model.midiActivity[$0] } ?? 0)
                    .frame(width: MidiStripMath.meterWidth, height: throwLength + 2 * StripLayout.meterOverhang)
                    .padding(.top, ControlMetrics.faderCap.height / 2 - StripLayout.meterOverhang)
                Spacer(minLength: 0)
            }
        }
        .disabled(dest == nil)
        .onDisappear { if let dest { model.midiCancel(dest) } }
    }
}

/// The velocity of the notes going out, 0...1, falling (`midiActivity`).
private struct MidiActivityMeter: View {
    let activity: Double
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .bottom) {
                Rectangle().fill(theme.meter.background.color)
                Rectangle()
                    .fill(theme.meter.low.color)
                    .frame(height: g.size.height * min(max(activity, 0), 1))
            }
        }
        .accessibilityHidden(true)
    }
}

/// The volume, typed (0 to 127).
private struct MidiStripLevel: View {
    let model: MixerModel
    let dest: UInt64?
    let name: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let volume = dest.flatMap { model.midiMix($0) }?.volume
        HStack {
            Spacer(minLength: 0)
            NumberBox(
                text: MidiStripMath.volumeLabel(volume), accessibilityLabel: "Volume of \(name)",
                palette: theme.readoutBox,
                commit: { t in dest.map { model.typeMidiVolume($0, t) } ?? false },
                onRefuse: { model.document.editor.status = MidiStripMath.volumeHint })
                .frame(
                    width: ControlMetrics.hitSize(.numberBox).width,
                    height: ControlMetrics.hitSize(.numberBox).height)
                .readoutEdge(theme)
                .opacity(volume == nil ? MidiStripMath.dimmed : 1)
                .accessibilityIdentifier(ControlUI.numberBoxID(named: "midi-volume"))
                .help("Type a volume from 0 to 127")
            Spacer(minLength: 0)
        }
        .disabled(dest == nil)
    }
}

/// M (the destination's mute) and R (the track's arm). No solo (Ruling 4).
private struct MidiStripButtons: View {
    let model: MixerModel
    let dest: UInt64?
    let lane: LaneRow?
    let name: String
    @Environment(\.symbioticTheme) private var theme

    var body: some View {
        let muted = dest.flatMap { model.midiMix($0) }?.mute ?? false
        HStack(spacing: theme.spacing.xs) {
            StripToggle(letter: "M", on: muted, colour: theme.state.mute, label: "Mute \(name)") {
                if let dest { model.toggleMidiMute(dest) }
            }
            .disabled(dest == nil)
            .help(dest == nil ? "Choose a destination first" : "Mute")
            StripToggle(letter: "R", on: lane?.arm == true, colour: theme.state.record, label: "Record arm \(lane?.name ?? name)") {
                if let lane { model.toggleMidiArm(lane) }
            }
            .disabled(lane == nil)
            .help(lane == nil ? "No track in this sequence plays this destination." : "Record arm")
        }
        .frame(maxWidth: .infinity)
    }
}
