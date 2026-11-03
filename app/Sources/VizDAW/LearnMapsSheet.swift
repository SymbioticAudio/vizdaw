import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The MIDI Learn Maps sheet (plan 5f-1 Task 11, Rulings 6, 7, 18): each
/// map's source, what it drives (or "(missing)"), its mode, its pick-up
/// and Remove. Every change is an undo step of the project. All state
/// lives in `LearnModel`.
struct LearnMapsSheet: View {
    @Bindable var learn: LearnModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("MIDI Learn Maps").font(.headline)
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
        .padding()
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
                    .font(.caption)
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
/// at the top of a window while a learn waits; nothing otherwise.
struct LearnBanner: View {
    let learn: LearnModel

    var body: some View {
        if let text = learn.banner {
            HStack {
                Image(systemName: "pianokeys")
                Text(text)
                Spacer()
                Button("Cancel") { learn.cancel() }
                    .controlSize(.small)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(Color.accentColor.opacity(0.15))
        }
    }
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
