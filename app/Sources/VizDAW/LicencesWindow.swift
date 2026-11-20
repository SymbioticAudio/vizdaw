import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel

/// VizDAW › Licences… (release gate Task 4): the bundle's licence files on
/// the left, the chosen one's text on the right, read-only and selectable.
struct LicencesView: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: LicencesModel

    var body: some View {
        HSplitView {
            List(model.entries, selection: $model.selection) { entry in
                Text(entry.title)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.text.primary.color)
                    .lineLimit(1)
                    .help(entry.id)
                    .accessibilityLabel(entry.title)
            }
            .frame(minWidth: 200, idealWidth: 240, maxWidth: 360)
            .accessibilityLabel("Licence files")
            LicenceText(text: model.text, theme: theme)
                .frame(minWidth: 420)
                .accessibilityLabel("Licence text")
        }
        .background(theme.surface.window.color)
        .frame(minWidth: 680, minHeight: 420)
    }
}

/// A read-only, selectable text view: AppKit's, since THIRD-PARTY.txt runs
/// to some hundred kilobytes, which a SwiftUI `Text` lays out slowly.
private struct LicenceText: NSViewRepresentable {
    let text: String
    let theme: Theme

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        if let view = scroll.documentView as? NSTextView {
            view.isEditable = false
            view.isSelectable = true
            view.isRichText = false
            view.textContainerInset = NSSize(width: theme.spacing.s, height: theme.spacing.s)
            view.setAccessibilityLabel("Licence text")
        }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? NSTextView else { return }
        scroll.backgroundColor = theme.surface.inset.nsColor
        view.backgroundColor = theme.surface.inset.nsColor
        let font = theme.type.body.nsFont
        let color = theme.text.primary.nsColor
        if view.string != text || view.textColor != color || view.font != font {
            view.string = text
            view.font = font
            view.textColor = color
            view.scrollToBeginningOfDocument(nil)
        }
    }
}
