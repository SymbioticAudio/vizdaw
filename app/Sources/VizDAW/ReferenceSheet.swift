import SwiftUI
import VizDAWAppModel

/// A reference's per-use length, repeats and transpose (spec §4a "set params").
struct ReferenceSheet: View {
    @Bindable var editor: EditorModel

    private func binding<T>(_ path: WritableKeyPath<ReferenceDraft, T>, _ fallback: T) -> Binding<T> {
        Binding(
            get: { editor.referenceDraft?[keyPath: path] ?? fallback },
            set: { editor.referenceDraft?[keyPath: path] = $0 })
    }

    var body: some View {
        Form {
            Toggle("Use the sequence's length", isOn: binding(\.useSequenceLength, true))
            Stepper(
                "Length: \(editor.referenceDraft?.lengthBeats ?? 1) beats",
                value: binding(\.lengthBeats, 1), in: 1...9_999
            )
            .disabled(editor.referenceDraft?.useSequenceLength ?? true)
            Toggle("Fill the length with repeats", isOn: binding(\.fill, false))
            Stepper("Repeats: \(editor.referenceDraft?.loops ?? 1)", value: binding(\.loops, 1), in: 1...999)
                .disabled(editor.referenceDraft?.fill ?? false)
            Stepper(
                "Transpose: \(editor.referenceDraft?.transpose ?? 0) semitones",
                value: binding(\.transpose, 0), in: -48...48)
            HStack {
                Spacer()
                Button("Cancel") { editor.referenceDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Apply") { editor.commitReference() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
        .frame(width: 340)
    }
}
