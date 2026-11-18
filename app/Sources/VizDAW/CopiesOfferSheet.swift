import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// "Remove Old Co-Producer Copies?" (plan 8b Ruling 8), after Compact from
/// the Media Pool: one toggle per copy, oldest first, none ticked. Keep All
/// is the default and Esc; Remove Selected sends only what the composer
/// ticked. All state lives in `CopiesOffer` and `MediaModel`.
struct CopiesOfferSheet: View {
    @Environment(\.symbioticTheme) private var theme
    let media: MediaModel
    @Bindable var offer: CopiesOffer

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Remove Old Co-Producer Copies?")
                .font(theme.type.title.font)
                .foregroundStyle(theme.text.primary.color)
            Text(offer.message)
                .font(theme.type.body.font)
                .foregroundStyle(theme.text.secondary.color)
                .fixedSize(horizontal: false, vertical: true)
            ScrollView {
                VStack(alignment: .leading, spacing: theme.spacing.xs) {
                    ForEach(offer.rows) { row in
                        Toggle(isOn: Binding(get: { row.chosen }, set: { _ in offer.toggle(row.id) })) {
                            HStack(spacing: theme.spacing.s) {
                                Text(row.id)
                                    .font(theme.type.body.font)
                                    .foregroundStyle(theme.text.primary.color)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                                Spacer(minLength: theme.spacing.s)
                                Text(row.size)
                                    .font(theme.type.caption.font)
                                    .foregroundStyle(theme.text.secondary.color)
                                    .monospacedDigit()
                            }
                        }
                        .toggleStyle(.checkbox)
                        .accessibilityLabel("\(row.id), \(row.size)")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 80, maxHeight: 260)
            .accessibilityLabel("Old Co-Producer copies")
            HStack(spacing: theme.spacing.s) {
                Spacer()
                // Keep All is both Return and Esc: one shortcut per button,
                // so Esc's is an unseen twin.
                Button("Keep All") { media.copiesOffer = nil }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Keep All")
                    .background {
                        Button("Keep All") { media.copiesOffer = nil }
                            .keyboardShortcut(.cancelAction)
                            .opacity(0)
                            .accessibilityHidden(true)
                    }
                Button("Remove Selected", role: .destructive) { media.removeChosenCopies() }
                    .disabled(!offer.canRemove)
                    .accessibilityLabel("Remove Selected")
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 520)
    }
}
