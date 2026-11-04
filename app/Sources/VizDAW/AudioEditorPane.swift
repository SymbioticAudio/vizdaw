import AppKit
import SwiftUI
import SymbioticControls
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The bottom pane's audio editor (plan 7c Task 6): a bar of typed values,
/// fade shapes, Normalize, zoom and waveform height over the canvas; with
/// no single audio region selected, the message. The root is the
/// "Audio Editor Pane" container the control channel finds (Ruling 10).
/// There is no pan control (Ruling 11).
struct AudioEditorPane: View {
    let model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            if model.editedRegion == nil {
                ContentUnavailableView(
                    "Audio Editor", icon: .audio, description: Text(AudioEditorModel.emptyMessage))
            } else {
                AudioEditorBar(model: model, editor: model.audioEditor)
                Divider()
                AudioEditorHost(model: model)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Audio Editor Pane")
        .accessibilityIdentifier("Audio Editor Pane")
    }
}

/// The canvas, in the pane's theme.
struct AudioEditorHost: NSViewRepresentable {
    let model: DocumentModel

    func makeNSView(context: Context) -> AudioEditorCanvas { AudioEditorCanvas(document: model) }

    func updateNSView(_ view: AudioEditorCanvas, context: Context) {
        view.theme = context.environment.symbioticTheme
    }
}

/// Gain, the fades and their shapes, start and length, then the peak and
/// Normalize, the zoom and the waveform height. In a horizontal scroll
/// view, so a narrow pane scrolls rather than squashes.
struct AudioEditorBar: View {
    let model: DocumentModel
    let editor: AudioEditorModel
    @Environment(\.symbioticTheme) private var theme

    private var boxSize: CGSize { ControlMetrics.hitSize(.numberBox) }

    private func title(_ text: String) -> some View {
        Text(text)
            .font(theme.type.label.font)
            .foregroundStyle(.secondary)
            .fixedSize()
            .accessibilityHidden(true)
    }

    /// A typed box (Ruling 7): Return commits, Escape or clicking away
    /// cancels; a refusal says the box's hint in the status line.
    private func box(_ b: AudioEditorBox) -> some View {
        NumberBox(
            text: editor.text(b), accessibilityLabel: b.label, palette: theme.readoutBox,
            commit: { editor.type(b, $0) },
            onRefuse: { model.editor.status = b.hint })
            .frame(width: boxSize.width, height: boxSize.height)
            .accessibilityIdentifier(ControlUI.numberBoxID)
    }

    private func field(_ text: String, _ b: AudioEditorBox) -> some View {
        HStack(spacing: theme.spacing.xs) {
            title(text)
            box(b)
        }
    }

    /// The fade's shape: a change keeps its length.
    private func shape(_ b: AudioEditorBox, _ label: String) -> some View {
        Picker(
            label,
            selection: Binding(
                get: {
                    let row = model.editedRegion?.row
                    return (b == .fadeIn ? row?.fadeIn.shape : row?.fadeOut.shape) ?? .linear
                },
                set: { editor.setShape(b, $0) })
        ) {
            ForEach(RegionHandles.shapes, id: \.1) { Text($0.1).tag($0.0) }
        }
        .labelsHidden()
        .fixedSize()
        .accessibilityLabel(label)
        .help(label)
    }

    private func icon(_ icon: AppIcon, _ label: String, _ run: @escaping () -> Void) -> some View {
        Button(action: run) { icon.image }
            .accessibilityLabel(label)
            .help(label)
    }

    private var row: some View {
        HStack(spacing: theme.spacing.m) {
            field("Gain", .gain)
            HStack(spacing: theme.spacing.xs) {
                field("Fade In", .fadeIn)
                shape(.fadeIn, "Fade In Shape")
            }
            HStack(spacing: theme.spacing.xs) {
                field("Fade Out", .fadeOut)
                shape(.fadeOut, "Fade Out Shape")
            }
            field("Start", .start)
            field("Length", .length)
            Spacer(minLength: theme.spacing.l)
            HStack(spacing: theme.spacing.xs) {
                field("Peak", .peak)
                Button("Normalize") { editor.normalize() }
                    .help("Normalize the region's gain to the peak")
            }
            HStack(spacing: theme.spacing.xs) {
                icon(.zoomOut, "Zoom Out") { editor.zoomOut() }
                icon(.zoomIn, "Zoom In") { editor.zoomIn() }
                Button("Fit Region") { editor.fit() }
                    .help("Fit the region to the pane")
            }
            HStack(spacing: theme.spacing.xs) {
                icon(.collapse, "Shorter Waveform") { editor.shorter() }
                icon(.expand, "Taller Waveform") { editor.taller() }
            }
        }
        .controlSize(.small)
        .padding(.horizontal, theme.spacing.s)
    }

    var body: some View {
        GeometryReader { g in
            ScrollView(.horizontal, showsIndicators: false) {
                row.frame(minWidth: g.size.width, minHeight: g.size.height)
            }
        }
        .frame(height: boxSize.height + 2 * theme.spacing.s)
    }
}
