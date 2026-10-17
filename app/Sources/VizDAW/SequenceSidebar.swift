import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The sequence sidebar (parent spec §9): name, keyboard slot, comment and
/// use count per sequence; drag to reorder; duplicate, delete, comment,
/// slot and new-from-template from the context menu and the "+" menu.
struct SequenceSidebar: View {
    @Bindable var model: DocumentModel

    var body: some View {
        List(selection: Binding(get: { model.selectedSeq }, set: { model.choose($0) })) {
            Section("Top-level") {
                ForEach(model.topLevel) { row in SequenceRow(model: model, row: row) }
                    .onMove { model.moveSequences(section: model.topLevel, from: $0, to: $1) }
            }
            Section("Used as reference") {
                ForEach(model.referenced) { row in SequenceRow(model: model, row: row) }
                    .onMove { model.moveSequences(section: model.referenced, from: $0, to: $1) }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            HStack {
                Menu {
                    Button("New Sequence") { model.newSequence(.empty) }
                    TemplateItems(model: model)
                } label: {
                    Image(systemName: "plus")
                }
                .menuStyle(.borderlessButton)
                .frame(width: 32)
                .help("New Sequence")
                Spacer()
            }
            .padding(6)
        }
        .sheet(item: $model.commentDraft) { _ in CommentSheet(model: model) }
        .sheet(item: $model.slotDraft) { _ in KeySlotSheet(model: model) }
        .confirmationDialog(
            "Delete this sequence and every reference to it?",
            isPresented: Binding(
                get: { model.pendingDelete != nil }, set: { if !$0 { model.pendingDelete = nil } })
        ) {
            Button("Delete", role: .destructive) { model.confirmDelete() }
            Button("Cancel", role: .cancel) { model.pendingDelete = nil }
        } message: {
            Text("Other sequences use it; their references are removed too. Undo brings both back.")
        }
    }
}

/// The "New from Template" items (Plan Ruling 13).
struct TemplateItems: View {
    let model: DocumentModel

    var body: some View {
        Menu("New from Template") {
            Button("Empty") { model.newSequence(.empty) }
            Button("Drum Kit") { model.newSequence(.drumKit) }
            Divider()
            ForEach(model.snapshot.sequences) { s in
                Button("Same Lanes as \(s.name)") { model.newSequence(.sameLanesAs(seq: s.id)) }
            }
        }
    }
}

struct SequenceRow: View {
    @Bindable var model: DocumentModel
    let row: SeqRow
    @FocusState private var focused: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 1) {
                if model.renamingSeq == row.id {
                    TextField("Name", text: $model.renameDraft)
                        .focused($focused)
                        .onSubmit { model.commitRename(model.renameDraft) }
                        .onExitCommand { model.cancelRename() }
                        .onAppear { focused = true }
                        .onChange(of: focused) { _, now in
                            if !now { model.commitRename(model.renameDraft) }
                        }
                } else {
                    Text(row.name)
                        .onTapGesture(count: 2) { model.beginRename(row.id) }
                }
                if !row.comment.isEmpty {
                    Text(row.comment)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer()
            if row.keyboardSlot != nil {
                Text(KeySlotFormat.text(row.keyboardSlot))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .help("Keyboard slot")
            }
            if row.useCount > 0 {
                Text("\(row.useCount)")
                    .font(.caption.monospacedDigit())
                    .padding(.horizontal, 6)
                    .background(Capsule().fill(.quaternary))
                    .help("Used by \(row.useCount) reference(s)")
            }
        }
        .tag(row.id)
        .accessibilityElement(children: .combine)
        .contextMenu {
            Button("Rename") { model.beginRename(row.id) }
            Button("Duplicate") { model.duplicate(row.id) }
            Button("Comment…") { model.beginComment(row.id) }
            Button("Keyboard Slot…") { model.beginSlot(row.id) }
            TemplateItems(model: model)
            Divider()
            Button("Delete…", role: .destructive) { model.requestDelete(row.id) }
        }
    }
}

struct CommentSheet: View {
    @Bindable var model: DocumentModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Comment").font(.headline)
            TextEditor(
                text: Binding(
                    get: { model.commentDraft?.text ?? "" },
                    set: { model.commentDraft?.text = $0 })
            )
            .frame(width: 320, height: 120)
            HStack {
                Spacer()
                Button("Cancel") { model.commentDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Save") { model.commitComment() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
    }
}

struct KeySlotSheet: View {
    @Bindable var model: DocumentModel

    private func flag(_ title: String, _ path: WritableKeyPath<KeySlotDraft, Bool>) -> some View {
        Toggle(
            title,
            isOn: Binding(
                get: { model.slotDraft?[keyPath: path] ?? false },
                set: { model.slotDraft?[keyPath: path] = $0 }))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Keyboard Slot").font(.headline)
            TextField(
                "Key (empty clears the slot)",
                text: Binding(get: { model.slotDraft?.key ?? "" }, set: { model.slotDraft?.key = $0 }))
            HStack {
                flag("⌃", \.control)
                flag("⌥", \.option)
                flag("⇧", \.shift)
                flag("⌘", \.command)
            }
            HStack {
                Spacer()
                Button("Cancel") { model.slotDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Save") { model.commitSlot() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
        .frame(width: 320)
    }
}
