import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// The MIDI Learn Maps sheet (plan 5f-1 Task 11, Rulings 6, 7, 18): each
/// map's source, what it drives (or "(missing)"), its mode, its pick-up
/// and Remove. Every change is an undo step of the project. All state
/// lives in `LearnModel`.
struct LearnMapsSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var learn: LearnModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("MIDI Learn Maps").font(theme.type.title.font)
            if learn.maps.isEmpty {
                Text("No MIDI controls are learned in this project. Choose MIDI Learn in a control's menu.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
            } else {
                List {
                    ForEach(learn.maps, id: \.source) { map in
                        row(map)
                    }
                }
                .frame(minHeight: 160)
            }
            HStack {
                Spacer()
                Button("Done") { learn.showsMaps = false }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 640)
        .alert(
            learn.alert?.title ?? "",
            isPresented: Binding(get: { learn.alert != nil }, set: { if !$0 { learn.alert = nil } })
        ) {
            Button("OK") { learn.alert = nil }
        } message: {
            Text(learn.alert?.message ?? "")
        }
    }

    private func row(_ map: LearnMapRow) -> some View {
        HStack {
            VStack(alignment: .leading) {
                Text(map.text)
                Text(learn.targetText(map))
                    .font(theme.type.caption.font)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Picker(
                "Mode",
                selection: Binding(get: { map.mode }, set: { learn.setMode(map, $0) })
            ) {
                ForEach(LearnModel.modes, id: \.self) { Text(LearnModel.modeTitle($0)).tag($0) }
            }
            .labelsHidden()
            .frame(width: 190)
            // Ruling 18: enabled where pick-up applies, dimmed and unticked
            // elsewhere.
            Toggle(
                "Pick-up",
                isOn: Binding(get: { map.picksUp && map.pickup }, set: { learn.setPickup(map, $0) })
            )
            .disabled(!map.picksUp)
            .help(LearnModel.pickupHelp)
            Button("Remove") { learn.remove(map) }
        }
    }
}

/// "Waiting for MIDI… move a control (Esc cancels)" with a Cancel button,
/// at the top of a window while a learn waits; nothing otherwise. It names
/// what the learn waits for and, with two or more projects open, the
/// project (final review R2, T11 m9).
struct LearnBanner: View {
    @Environment(\.symbioticTheme) private var theme
    let learn: LearnModel
    /// The learn's document, named when two or more are open.
    let document: DocumentModel?

    var body: some View {
        if let text = learn.bannerText(project: learnProject(document)) {
            HStack {
                AppIcon.midiLearn.image
                Text(text)
                Spacer()
                Button("Cancel") { learn.cancel() }
                    .controlSize(.small)
            }
            .padding(.horizontal, theme.spacing.s)
            .padding(.vertical, theme.spacing.xs)
            .background(theme.accent.accent.color.opacity(0.15))
        }
    }
}

/// The project a banner names: the document's title when two or more
/// documents are open, else nil (final review R2).
@MainActor func learnProject(_ document: DocumentModel?) -> String? {
    guard let document, appStore.documents.count >= 2 else { return nil }
    return document.windowTitle
}

extension View {
    /// A refused learn ("MIDI Learn Failed") as an alert, while `shown`
    /// (the mixer leaves it to the open maps sheet).
    func learnAlert(_ learn: LearnModel, shown: Bool = true) -> some View {
        alert(
            learn.alert?.title ?? "",
            isPresented: Binding(get: { shown && learn.alert != nil }, set: { if !$0 { learn.alert = nil } })
        ) {
            Button("OK") { learn.alert = nil }
        } message: {
            Text(learn.alert?.message ?? "")
        }
    }
}
