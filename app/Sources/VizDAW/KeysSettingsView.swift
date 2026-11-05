import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// Settings > Keys (plan 4f Ruling 17): every command by menu with its
/// keys; record a new key, reset one or all, save and load a bindings
/// file, and keep the number row for locators.
struct KeysSettingsView: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var keys: KeyBindings
    let store: DocumentStore

    /// The document "Learn MIDI" learns into (plan 5f-1 Ruling 11): the
    /// command target, else the front-most document window still open
    /// (Settings is key, so no document window is; final review R2); nil
    /// without a document.
    private var document: DocumentModel? {
        store.noteTarget ?? store.frontDocument
    }

    private var groups: [(String, [BindingRow])] {
        var order: [String] = []
        var by: [String: [BindingRow]] = [:]
        for r in keys.rows {
            if by[r.menu] == nil { order.append(r.menu) }
            by[r.menu, default: []].append(r)
        }
        return order.map { ($0, by[$0] ?? []) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle("Number keys always go to locators (step entry uses the keypad)", isOn: $keys.reserveNumberRow)
            if let learn = document?.learn {
                LearnBanner(learn: learn, document: document)
            }
            if let m = keys.message {
                HStack {
                    Text(m).font(theme.type.caption.font).foregroundStyle(.secondary).lineLimit(3)
                    if keys.pending != nil {
                        Button("Replace") { keys.confirmReplace() }
                        Button("Cancel") { keys.cancelReplace() }
                    }
                }
            }
            List {
                ForEach(groups, id: \.0) { group in
                    Section(group.0) {
                        ForEach(group.1, id: \.id) { row in
                            HStack {
                                Text(row.title)
                                if row.stepEntry {
                                    Text("step entry").font(theme.type.caption.font).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(row.keys.joined(separator: "  "))
                                    .font(theme.type.body.font.monospaced())
                                    .foregroundStyle(
                                        row.keys == row.defaults ? theme.text.primary.color : theme.accent.accent.color)
                                KeyRecorder(title: row.title) { keys.record(row.id, key: $0) }
                                    .frame(width: 96, height: 20)
                                learnButton(row)
                                Button("Reset") { keys.reset(row.id) }
                                    .disabled(row.keys == row.defaults)
                            }
                        }
                    }
                }
            }
            HStack {
                Button("Save…") { save() }
                Button("Load…") { load() }
                Spacer()
                Button("Reset All") { keys.resetAll() }
            }
        }
        .padding()
        .frame(width: 720, height: 520)
        .modifier(KeysLearnAlert(document: document))
    }

    /// "Learn MIDI": the next MIDI control moved runs this command
    /// (Ruling 8: only commands a key can run; Ruling 11: only with a
    /// document open).
    private func learnButton(_ row: BindingRow) -> some View {
        let doc = document
        return Button("Learn MIDI") { doc?.learn.learnCommand(row.id) }
            .disabled(doc == nil || !row.learnable)
            .help(
                doc == nil
                    ? "Open a project to learn MIDI"
                    : row.learnable
                        ? "Learn a MIDI control (a pad or button) for \(row.title)"
                        : "Only commands a key can run can be learned")
    }

    private func save() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "Key Bindings.vizkeys"
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        do {
            try keys.save(to: path)
        } catch {
            keys.message = AlertItem(error, title: "").message
        }
    }

    private func load() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        try? keys.load(from: path)
    }
}

/// A refused "Learn MIDI" ("MIDI Learn Failed").
private struct KeysLearnAlert: ViewModifier {
    let document: DocumentModel?

    func body(content: Content) -> some View {
        if let learn = document?.learn {
            content.learnAlert(learn, shown: !learn.showsMaps)
        } else {
            content
        }
    }
}

/// The AppKit key recorder in a row.
struct KeyRecorder: NSViewRepresentable {
    let title: String
    let onKey: (String) -> Void

    func makeNSView(context: Context) -> KeyRecorderView {
        let v = KeyRecorderView()
        v.title = title
        v.onKey = onKey
        v.theme = context.environment.symbioticTheme
        return v
    }

    /// In the theme of its root (B4 review M1: it was never themed).
    func updateNSView(_ v: KeyRecorderView, context: Context) {
        v.title = title
        v.onKey = onKey
        v.theme = context.environment.symbioticTheme
    }
}
