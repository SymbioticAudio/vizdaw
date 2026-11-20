import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// A menu bar stand-in: it records what a command button asked of it.
@MainActor
private final class FakeMenu: MenuPerformer {
    var asked: [(menu: String, title: String)] = []
    var answers = true

    func perform(menu: String, title: String) -> Bool {
        asked.append((menu, title))
        return answers
    }
}

/// Customize Transport Bar: the palette, the bar's draft and command
/// buttons (groups-vcas-transport Task 12; Rulings 13, 15, 16 and 17).
@MainActor
@Suite struct TransportCustomizeTests {
    private func model(_ items: String? = nil) -> (TransportCustomizeModel, TransportLayoutStore, MemoryDefaults) {
        let d = MemoryDefaults()
        let store = TransportLayoutStore(defaults: d)
        if let items, case .success(let l) = TransportLayout.parse(items) { store.set(l) }
        return (TransportCustomizeModel(store: store), store, d)
    }

    private func demo() throws -> (DocumentStore, UUID, DocumentModel) {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m)
    }

    @Test func customizeCannotRemoveTheCluster() {
        let (c, _, _) = model("loop,cluster,bar")
        let before = c.draft
        #expect(!c.remove(at: 1))
        #expect(c.draft == before)
        #expect(!c.remove(at: 99) && !c.remove(at: -1), "out of range")
        #expect(c.draft == before)
    }

    @Test func customizeOpsKeepOrder() {
        let (c, store, _) = model("cluster,loop,bar")
        c.add(.builtIn(.tempo, nil), at: 1)
        #expect(c.draft.text == "cluster,tempo,loop,bar")
        c.add(.builtIn(.space, nil), at: 99)
        #expect(c.draft.text == "cluster,tempo,loop,bar,space", "past the end appends")
        c.add(.builtIn(.loop, nil), at: 0)
        #expect(c.draft.text == "cluster,tempo,loop,bar,space", "a built-in item is on the bar once")
        c.move(from: 0, to: 3)
        #expect(c.draft.text == "tempo,loop,bar,cluster,space")
        c.move(from: 4, to: 0)
        #expect(c.draft.text == "space,tempo,loop,bar,cluster")
        #expect(c.remove(at: 2))
        #expect(c.draft.text == "space,tempo,bar,cluster")
        c.setSize(at: 2, .small)
        c.setSize(at: 0, .small)
        #expect(c.draft.text == "space,tempo,bar:small,cluster", "only bar, time and tempo come small")
        c.cancel()
        #expect(store.layout.text == "cluster,loop,bar", "cancel leaves the bar")
        let (d, store2, _) = model("cluster,loop,bar")
        #expect(d.remove(at: 1))
        d.apply()
        #expect(store2.layout.text == "cluster,bar", "apply writes it")
    }

    @Test func paletteOffersWhatIsNotOnTheBar() {
        let (c, _, _) = model("cluster,loop,space,bar:small")
        let builtIns = c.palette.filter {
            if case .builtIn(let id, _) = $0 { return !id.repeats }
            return false
        }
        #expect(!builtIns.contains { [.cluster, .loop, .bar].contains($0.id) })
        let others = TransportItemID.allCases.filter { ![.cluster, .loop, .bar, .space, .separator, .command].contains($0) }
        #expect(builtIns.map(\.id) == others, "every other built-in, in order")
        #expect(Array(c.palette.suffix(3).map(\.id)) == [.space, .separator, .command], "then space, separator, a command")
        c.add(.builtIn(.tempo, nil), at: 1)
        #expect(!c.palette.contains { $0.id == .tempo })
        #expect(c.palette.contains { $0.id == .space }, "spaces are always offered")
        #expect(TransportCustomizeEntries.palette(c.palette).map(\.title).contains("Command Button"))
    }

    @Test func commandPickerFindsByTitleAndMenu() {
        let rows = KeyBindings(defaults: MemoryDefaults()).rows
        #expect(CommandPicker.rows(rows, query: "mix").contains { $0.id == "window.mixer" })
        let file = CommandPicker.rows(rows, query: "File")
        #expect(file.contains { $0.id == "file.save" })
        #expect(CommandPicker.rows(rows, query: "") == rows)
        #expect(CommandPicker.rows(rows, query: "  ") == rows)
        #expect(CommandPicker.rows(rows, query: "WINDOW.MIXER").map(\.id) == ["window.mixer"], "by id, any case")
        let order = rows.map(\.id)
        let found = file.map(\.id)
        #expect(found == order.filter { found.contains($0) }, "registry order")
        #expect(CommandPicker.rows(rows, query: String(repeating: "x", count: 10_000)).isEmpty)
    }

    @Test func commandLabelsAreTrimmedAndBounded() {
        let (c, store, d) = model("cluster,command:window.mixer")
        #expect(c.setCommand(at: 1, id: "window.mixer", label: "  Go  "))
        #expect(c.draft.items[1] == .command(id: "window.mixer", label: "Go"))
        #expect(!c.setCommand(at: 1, id: "window.mixer", label: String(repeating: "a", count: 33)))
        #expect(c.draft.items[1] == .command(id: "window.mixer", label: "Go"), "33 characters are refused")
        #expect(c.setCommand(at: 1, id: "window.mixer", label: String(repeating: "a", count: 32)))
        #expect(c.setCommand(at: 1, id: "file.save", label: "   "))
        #expect(c.draft.items[1] == .command(id: "file.save", label: nil), "blank is the title")
        #expect(!c.setCommand(at: 0, id: "file.save", label: nil), "not a command button")
        #expect(!c.setCommand(at: 1, id: "bad id,x", label: nil), "not an id")
        // A label with the bar text's comma and colon goes through the
        // text and the preference.
        #expect(c.setCommand(at: 1, id: "file.save", label: "Save, then: go 100%"))
        c.apply()
        #expect(TransportLayout.parse(store.layout.text) == .success(store.layout))
        #expect(TransportLayoutStore(defaults: d).layout.items[1] == .command(id: "file.save", label: "Save, then: go 100%"))
        for bad in ["command", "command:", "command:file.save: ", "command:FILE SAVE", "command:file.save:" + String(repeating: "a", count: 33)] {
            #expect(throws: Never.self) { _ = TransportLayout.parse("cluster," + bad) }
            if case .success = TransportLayout.parse("cluster," + bad) { Issue.record("\(bad) was taken") }
        }
    }

    @Test func staleCommandsStayGreyed() throws {
        let (c, store, d) = model("cluster,command:gone.command")
        let reloaded = TransportLayoutStore(defaults: d)
        #expect(reloaded.layout.items == [.builtIn(.cluster, nil), .command(id: "gone.command", label: nil)])
        let keys = KeyBindings(defaults: MemoryDefaults())
        let (_, _, m) = try demo()
        let face = CommandButtonFace.make(id: "gone.command", label: nil, row: keys.row("gone.command"), doc: m)
        #expect(!face.enabled && face.stale)
        #expect(face.tooltip == CommandButtonAction.staleTooltip)
        #expect(face.label == "gone.command", "the id when there is no label")
        let labelled = CommandButtonFace.make(id: "gone.command", label: "Old", row: nil, doc: m)
        #expect(labelled.label == "Old" && !labelled.enabled)
        let live = CommandButtonFace.make(id: "window.mixer", label: nil, row: keys.row("window.mixer"), doc: m)
        #expect(live.enabled && !live.stale && live.label == "Mixer" && live.tooltip == "Mixer")
        #expect(live.accessibilityLabel == "Mixer" && live.icon == .mixer)
        // Folded into », it is the same, greyed.
        let entry = try #require(
            TransportItemEntries.overflow(
                .command(id: "gone.command", label: nil), m, documentWindow: true, commandRow: { keys.row($0) }))
        #expect(entry.title == "gone.command" && !entry.enabled)
        c.resetToDefault()
        c.apply()
        #expect(store.layout == .standard)
        #expect(!TransportLayoutStore(defaults: d).layout.items.contains { $0.id == .command })
    }

    @Test func commandAvailabilityFollowsTheMenus() throws {
        let empty = try DocumentModel(session: FakeSession([]))
        #expect(!CommandAvailability.enabled("transport.play", doc: empty))
        let (_, _, m) = try demo()
        #expect(CommandAvailability.enabled("transport.play", doc: m))
        #expect(CommandAvailability.enabled("window.mixer", doc: m))
        for id in ["window.mixer", "file.save", "transport.play", "track.new_audio", "region.normalize"] {
            #expect(!CommandAvailability.enabled(id, doc: nil), "\(id)")
        }
        #expect(CommandAvailability.enabled("file.new", doc: nil), "an app-wide command needs no document")
        #expect(CommandGlyphs.icon("editors.piano_roll") == .pianoRoll)
        #expect(CommandGlyphs.icon("editors.list") == .list)
        #expect(CommandGlyphs.icon("editors.drum_grid") == .drumGrid)
        #expect(CommandGlyphs.icon("transport.loop") == .loop)
        #expect(CommandGlyphs.icon("transport.record") == .record)
        #expect(CommandGlyphs.icon("file.save") == nil)
    }

    @Test func aDialogCommandFallsThroughToItsMenuItem() throws {
        let (s, id, m) = try demo()
        let host = FakeControlHost()
        let menu = FakeMenu()
        CommandButtonAction.run("file.bounce", doc: (id, m), store: s, host: host, menu: menu)
        #expect(menu.asked.count == 1)
        #expect(menu.asked.first?.menu == "File" && menu.asked.first?.title == "Bounce…")
        CommandButtonAction.run("window.mixer", doc: (id, m), store: s, host: host, menu: menu)
        #expect(menu.asked.count == 1, "the mixer opens without the menu")
        #expect(host.calls.last == "mixer")
        menu.answers = false
        CommandButtonAction.run("file.bounce", doc: (id, m), store: s, host: host, menu: menu)
        #expect(m.editor.status == "Bounce… can't run from a button right now.")
        let before = menu.asked.count
        CommandButtonAction.run("gone.command", doc: (id, m), store: s, host: host, menu: menu)
        #expect(menu.asked.count == before, "a stale id asks nothing")
    }

    @Test func customizeTransportOverTheChannelNeedsTheDialog() throws {
        let (s, id, m) = try demo()
        #expect(ControlCommands.handled.contains("ui.customize_transport"))
        for doc in [nil, (id, m)] as [(id: UUID, model: DocumentModel)?] {
            guard case .failed(let f) = ControlCommands.run(
                "ui.customize_transport", args: [], doc: doc, store: s, host: FakeControlHost())
            else {
                Issue.record("it ran")
                continue
            }
            #expect(f.code == .dialogRequired)
        }
        #expect(s.transportLayout.customizing == nil && s.transportLayout.customizer == nil, "no sheet opens")
        // The View menu's item opens it on the window that asked.
        let w = UUID()
        s.transportLayout.beginCustomizing(window: w)
        #expect(s.transportLayout.customizing == w && s.transportLayout.customizer != nil)
        s.transportLayout.customizer?.cancel()
        #expect(s.transportLayout.customizing == nil && s.transportLayout.customizer == nil)
    }

    @Test func anInvalidLayoutIsNeverWritten() {
        let d = MemoryDefaults()
        let store = TransportLayoutStore(defaults: d)
        store.set(TransportLayout(items: [.builtIn(.cluster, nil), .builtIn(.loop, nil)]))
        let saved = d.string(forKey: TransportLayoutStore.key)
        for bad in [
            [.builtIn(.cluster, nil), .builtIn(.loop, nil), .builtIn(.loop, nil)],
            [.builtIn(.loop, nil)],
            [.builtIn(.cluster, nil), .builtIn(.loop, .small)],
            [.builtIn(.cluster, nil), .builtIn(.command, nil)],
            [.builtIn(.cluster, nil), .command(id: "a,b", label: nil)],
            [.builtIn(.cluster, nil), .command(id: "file.save", label: " padded ")],
            Array(repeating: TransportItem.builtIn(.space, nil), count: 64) + [.builtIn(.cluster, nil)],
        ] as [[TransportItem]] {
            #expect(store.set(TransportLayout(items: bad)) != nil, "\(bad)")
            #expect(d.string(forKey: TransportLayoutStore.key) == saved)
            #expect(store.layout.text == "cluster,loop")
        }
    }

    @Test func theSheetsEntriesAndDrops() {
        let (c, _, _) = model("cluster,loop,command:window.mixer")
        let keys = KeyBindings(defaults: MemoryDefaults())
        let bar = TransportCustomizeEntries.bar(c.draft.items) { keys.row($0) }
        #expect(bar.map(\.title) == ["Transport Controls", "Loop", "Mixer"])
        #expect(bar[0].actions == ["Move Right"], "the cluster is never removed")
        #expect(bar[1].actions == ["Remove", "Move Left", "Move Right"])
        #expect(bar[2].actions == ["Remove", "Move Left"])
        #expect(bar[2].accessibilityLabel == "Command Button: Mixer")
        #expect(TransportCustomizeEntries.palette(c.palette).allSatisfy { $0.actions == ["Add to Bar"] })
        // A palette item dropped on the bar, a bar item along it and back.
        let tempo = c.palette.firstIndex { $0.id == .tempo }!
        #expect(TransportCustomizeEntries.drop(TransportCustomizeEntries.token(palette: tempo), onBarAt: 1, c))
        #expect(c.draft.text == "cluster,tempo,loop,command:window.mixer")
        #expect(TransportCustomizeEntries.drop(TransportCustomizeEntries.token(bar: 0), onBarAt: 3, c))
        #expect(c.draft.text == "tempo,loop,cluster,command:window.mixer")
        #expect(!TransportCustomizeEntries.dropOnPalette(TransportCustomizeEntries.token(bar: 2), c), "not the cluster")
        #expect(TransportCustomizeEntries.dropOnPalette(TransportCustomizeEntries.token(bar: 3), c))
        #expect(c.draft.text == "tempo,loop,cluster")
        for hostile in ["", "bar:", "bar:-1", "bar:99999999999999999999", "palette:999", "x:1", "loop"] {
            #expect(!TransportCustomizeEntries.drop(hostile, onBarAt: 0, c), "\(hostile)")
            #expect(!TransportCustomizeEntries.dropOnPalette(hostile, c), "\(hostile)")
        }
        #expect(c.draft.text == "tempo,loop,cluster")
        // The inspector: a command button's label is typed, then taken.
        c.add(.command(id: "file.save", label: nil), at: 3)
        c.select(3)
        c.labelDraft = "  Save it "
        #expect(c.commitLabel())
        #expect(c.draft.items[3] == .command(id: "file.save", label: "Save it"))
        c.chooseCommand("window.mixer")
        #expect(c.draft.items[3] == .command(id: "window.mixer", label: "Save it"))
        c.move(from: 3, to: 0)
        #expect(c.selected == 0, "the selection follows its item")
    }

    @Test func commandButtonsForDifferentCommandsAreDifferentViews() throws {
        let a = TransportItem.command(id: "window.mixer", label: nil)
        let b = TransportItem.command(id: "file.save", label: nil)
        #expect(TransportRowView.key(a, 1) != TransportRowView.key(b, 1))
        let m = try DocumentModel(session: demoFake())
        let l = try TransportLayout.checked([.builtIn(.cluster, nil), a, b]).get()
        let row = TransportRowView(document: m, layout: l, documentWindow: true) { _ in nil }
        #expect(row.slots.count == 3)
        #expect(row.slots[1].view !== row.slots[2].view)
        let swapped = try TransportLayout.checked([.builtIn(.cluster, nil), b, a]).get()
        row.setLayout(swapped)
        #expect(row.slots.map(\.item) == swapped.items)
    }
}
