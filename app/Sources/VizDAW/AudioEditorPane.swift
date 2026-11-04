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
            .foregroundStyle(theme.text.secondary.color)
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

    /// The typed values.
    private var values: some View {
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
        }
    }

    /// The Peak box and Normalize.
    private var normalize: some View {
        HStack(spacing: theme.spacing.xs) {
            field("Peak", .peak)
            Button("Normalize") { editor.normalize() }
                .help("Normalize the region's gain to the peak")
        }
    }

    /// The zoom and the waveform height.
    private var viewControls: some View {
        HStack(spacing: theme.spacing.m) {
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
    }

    /// One row when the pane is wide enough; otherwise two (Task 6 review
    /// I1), as the transport bar wraps: the values on the first, scrolling
    /// with a visible indicator only if the pane is narrower still, and
    /// Peak, Normalize and the view controls on the second, never
    /// scrolled, the view controls at its trailing end.
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: theme.spacing.m) {
                values
                Spacer(minLength: theme.spacing.l)
                normalize
                viewControls
            }
            .padding(theme.spacing.s)
            VStack(spacing: 0) {
                ScrollView(.horizontal) {
                    values.padding(theme.spacing.s)
                }
                .scrollIndicators(.visible)
                HStack(spacing: theme.spacing.m) {
                    normalize
                    Spacer(minLength: theme.spacing.l)
                    viewControls
                }
                .padding([.horizontal, .bottom], theme.spacing.s)
            }
        }
        .controlSize(.small)
        .fixedSize(horizontal: false, vertical: true)
    }
}
