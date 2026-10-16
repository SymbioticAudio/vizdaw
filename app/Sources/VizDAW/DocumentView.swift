import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Sidebar of sequences, table of lanes, transport toolbar.
struct DocumentView: View {
    @Bindable var model: DocumentModel

    var body: some View {
        NavigationSplitView {
            SequenceSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } detail: {
            LaneTable(model: model)
        }
        .navigationTitle(model.windowTitle)
        .toolbar { TransportToolbar(model: model) }
        .alert(
            model.alert?.title ?? "",
            isPresented: Binding(
                get: { model.alert != nil },
                set: { if !$0 { model.alert = nil } }),
            presenting: model.alert
        ) { _ in
            Button("OK") { model.alert = nil }
        } message: { item in
            Text(item.message)
        }
    }
}

struct SequenceSidebar: View {
    @Bindable var model: DocumentModel

    var body: some View {
        List(selection: Binding(get: { model.selectedSeq }, set: { model.select($0) })) {
            Section("Top-level") {
                ForEach(model.topLevel) { row in SequenceRow(model: model, row: row) }
            }
            Section("Used as reference") {
                ForEach(model.referenced) { row in SequenceRow(model: model, row: row) }
            }
        }
        .listStyle(.sidebar)
    }
}

struct SequenceRow: View {
    @Bindable var model: DocumentModel
    let row: SeqRow
    @FocusState private var focused: Bool

    var body: some View {
        HStack {
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
            Spacer()
            if row.useCount > 0 {
                Text("\(row.useCount)")
                    .font(.caption.monospacedDigit())
                    .padding(.horizontal, 6)
                    .background(Capsule().fill(.quaternary))
                    .help("Used by \(row.useCount) reference(s)")
            }
        }
        .tag(row.id)
    }
}

struct LaneTable: View {
    let model: DocumentModel

    var body: some View {
        if model.selectedSeq == nil {
            ContentUnavailableView(
                "No Sequence", systemImage: "music.note.list",
                description: Text("Choose File > Create Demo Project to try VizDAW."))
        } else {
            Table(model.lanes) {
                TableColumn("Mute") { lane in
                    Toggle(
                        "Mute",
                        isOn: Binding(
                            get: { lane.mute },
                            set: { model.setMute(lane: lane.id, $0) })
                    )
                    .labelsHidden()
                }
                .width(40)
                TableColumn("Name", value: \.name)
                TableColumn("Destination") { Text($0.destination ?? "—") }
                TableColumn("MIDI events") { Text("\($0.midiEventCount)").monospacedDigit() }
                TableColumn("Regions") { Text("\($0.regionCount)").monospacedDigit() }
                TableColumn("References") { Text("\($0.referenceCount)").monospacedDigit() }
                TableColumn("Takes (active)") { Text("\($0.takeCount) (\($0.activeTake))") }
                TableColumn("Loop") { Text($0.looped ? "Yes" : "") }
                TableColumn("Drum") { Text($0.isDrum ? "Yes" : "") }
            }
        }
    }
}

struct TransportToolbar: ToolbarContent {
    @Bindable var model: DocumentModel

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .navigation) {
            Button {
                model.playStop()
            } label: {
                Label(
                    model.isPlaying ? "Stop" : "Play",
                    systemImage: model.isPlaying ? "stop.fill" : "play.fill")
            }
            .disabled(!model.canPlay)
            Toggle(
                isOn: Binding(get: { model.looping }, set: { _ in model.toggleLoop() })
            ) {
                Label("Loop", systemImage: "repeat")
            }
        }
        ToolbarItemGroup(placement: .principal) {
            Text(Formatting.position(model.transport))
                .font(.system(.title3, design: .monospaced))
                .frame(minWidth: 220)
        }
        ToolbarItemGroup(placement: .primaryAction) {
            Text(Formatting.device(model.transport))
                .font(.caption)
                .foregroundStyle(.secondary)
            if model.hasFaults || model.hasReport {
                Button {
                    model.showsFaults.toggle()
                } label: {
                    Image(systemName: "circle.fill")
                        .foregroundStyle(model.hasFaults ? .red : .orange)
                }
                .help("Faults and compile messages")
                .popover(isPresented: $model.showsFaults) {
                    VStack(alignment: .leading, spacing: 4) {
                        let lines = model.transport.faults + model.transport.report
                        ForEach(Array(lines.enumerated()), id: \.offset) {
                            Text($0.element)
                        }
                    }
                    .padding()
                    .frame(minWidth: 240)
                }
            }
        }
    }
}
