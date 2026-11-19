import AppKit
import Foundation
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// A stand-in for an app item's view (the app target's views are not
/// reachable from tests: pre-flight F9), of a fixed size.
private final class StandIn: NSView {
    let size: NSSize

    init(_ width: CGFloat, height: CGFloat = 22) {
        size = NSSize(width: width, height: height)
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { nil }

    override var intrinsicContentSize: NSSize { size }
}

/// The app items' stand-in widths: roughly the demo's.
private let appWidths: [TransportItemID: CGFloat] = [
    .cluster: 180, .loop: 30, .click: 64, .replaceOverdub: 72, .recordOptions: 30, .countIn: 70,
    // No step entry: the summary shows nothing.
    .summary: 0,
    .dspLoad: 60, .device: 110, .faults: 30, .mainView: 110, .panes: 100,
]

/// The transport bar as items on one row with » (groups-vcas-transport
/// Task 11; Rulings 13, 14, 17 and 18).
@MainActor
@Suite struct TransportRowTests {
    typealias Piece = TransportRowFit.Piece

    /// The standard layout's pieces at test widths.
    private func standardPieces() -> [Piece] {
        let widths: [TransportItemID: CGFloat] = [
            .cluster: 230, .loop: 32, .click: 70, .replaceOverdub: 80, .recordOptions: 32, .summary: 120,
            .bar: 80, .time: 90, .tempo: 70, .signature: 50, .positionField: 160, .editFields: 300,
            .punchFields: 300, .space: 0, .device: 150, .faults: 32, .mainView: 120, .panes: 100,
        ]
        return TransportLayout.standard.items.map { Piece(width: widths[$0.id] ?? 0, kind: TransportRowView.kind($0)) }
    }

    @Test func nothingIsCut() {
        let pieces = standardPieces()
        #expect(pieces.count == 18)
        let content = Set(pieces.indices.filter { pieces[$0].kind == .content })
        let cluster = pieces.firstIndex { $0.kind == .fixed }
        let spacing: CGFloat = 12
        let chevron: CGFloat = 28
        let widths = stride(from: CGFloat(0), through: 2000, by: 10).map { $0 } + [.nan, -1, .infinity]
        for w in widths {
            let (shown, folded) = TransportRowFit.fit(pieces, width: w, spacing: spacing, chevron: chevron)
            let shownContent = Set(shown).intersection(content)
            #expect(shownContent.union(folded) == content, "\(w): every content piece is shown or folded")
            #expect(shownContent.isDisjoint(with: folded), "\(w)")
            #expect(Set(folded).isSubset(of: content), "\(w): only content folds")
            #expect(shown == shown.sorted() && folded == folded.sorted(), "\(w): bar order")
            #expect(cluster.map(shown.contains) == true, "\(w): the cluster shows")
            guard w.isFinite else { continue }
            let need =
                TransportRowFit.used(shown, pieces, spacing: spacing) + (folded.isEmpty ? 0 : spacing + chevron)
            #expect(need <= w || shown == [cluster ?? -1], "\(w): \(need)")
        }
        #expect(TransportRowFit.fit(pieces, width: .infinity, spacing: spacing, chevron: chevron).folded.isEmpty)
        #expect(TransportRowFit.fit(pieces, width: .nan, spacing: spacing, chevron: chevron).folded.count == content.count)
        #expect(TransportRowFit.fit(pieces, width: 0, spacing: spacing, chevron: chevron).shown == [0])
    }

    @Test func foldsTheLastItemFirst() {
        let pieces = [
            Piece(width: 50, kind: .content), Piece(width: 100, kind: .fixed), Piece(width: 50, kind: .content),
            Piece(width: 50, kind: .content),
        ]
        func folded(_ w: CGFloat) -> [Int] {
            TransportRowFit.fit(pieces, width: w, spacing: 10, chevron: 20).folded
        }
        #expect(folded(300) == [])
        #expect(folded(260) == [3], "c first")
        #expect(folded(200) == [2, 3], "then b")
        #expect(folded(150) == [0, 2, 3], "then a")
        #expect(folded(100) == [0, 2, 3])
        #expect(TransportRowFit.fit(pieces, width: 100, spacing: 10, chevron: 20).shown == [1])
    }

    @Test func spacesAndSeparatorsAreNeverListed() {
        let pieces = [
            Piece(width: 100, kind: .fixed), Piece(width: 50, kind: .content), Piece(width: 1, kind: .separator),
            Piece(width: 0, kind: .space), Piece(width: 50, kind: .content), Piece(width: 1, kind: .separator),
            Piece(width: 1, kind: .separator), Piece(width: 50, kind: .content), Piece(width: 1, kind: .separator),
        ]
        for w in stride(from: CGFloat(0), through: 500, by: 5) {
            let (shown, folded) = TransportRowFit.fit(pieces, width: w, spacing: 10, chevron: 20)
            #expect(folded.allSatisfy { pieces[$0].kind == .content }, "\(w)")
            #expect(!shown.contains(8), "\(w): a separator at the edge never shows")
            #expect(!shown.contains(5) && !shown.contains(6), "\(w): two separators side by side never show")
        }
        // A separator next to » is hidden.
        let row = [
            Piece(width: 100, kind: .fixed), Piece(width: 50, kind: .content), Piece(width: 1, kind: .separator),
            Piece(width: 50, kind: .content),
        ]
        #expect(TransportRowFit.fit(row, width: 400, spacing: 10, chevron: 20).shown == [0, 1, 2, 3])
        let (shown, folded) = TransportRowFit.fit(row, width: 200, spacing: 10, chevron: 20)
        #expect(folded == [3])
        #expect(shown == [0, 1], "the separator before » is hidden")
    }

    @Test func theDefaultLayoutIsTodaysBar() {
        #expect(
            TransportLayout.standard.items.map(\.id) == [
                .cluster, .loop, .click, .replaceOverdub, .recordOptions, .summary, .bar, .time, .tempo, .signature,
                .positionField, .editFields, .punchFields, .space, .device, .faults, .mainView, .panes,
            ])
        // The mixer window's: no document-only item.
        #expect(!TransportLayout.standard.items(documentWindow: false).contains { $0.id.documentOnly })
        #expect(TransportItemID.allCases.filter(\.documentOnly) == [.faults, .mainView, .panes])
        #expect(TransportItemID.allCases.filter(\.sizes) == [.bar, .time, .tempo])
        #expect(TransportItemID.replaceOverdub.title == "Replace/Overdub" && TransportItemID.loop.title == "Loop")
        #expect(TransportItemID.bar.title == "Position in Bars")
        // Every item's snake_case name reads back.
        for id in TransportItemID.allCases { #expect(TransportItemID(token: id.token) == id, "\(id)") }
        #expect(TransportItemID.replaceOverdub.token == "replace_overdub")
    }

    /// Review Focus 4.
    @Test func aDamagedPreferenceGivesTheDefaultAndIsKept() throws {
        let key = TransportLayoutStore.key
        #expect(key == "VizDAW.transportBar")
        for bad in [
            "{not json", #"{"version":99,"items":[]}"#, #"{"version":1,"items":["loop"]}"#,
            #"{"version":1,"items":["cluster","clik"]}"#, #"{"items":["cluster"]}"#,
        ] {
            let d = MemoryDefaults()
            d.set(bad, forKey: key)
            let s = TransportLayoutStore(defaults: d)
            #expect(s.layout == .standard, "\(bad)")
            #expect(d.string(forKey: key) == bad, "\(bad) is kept")
            let mine = try TransportLayout.parse("cluster,loop,bar:large").get()
            s.set(mine)
            let written = try #require(d.string(forKey: key))
            #expect(written != bad, "rewritten once the bar changes")
            #expect(TransportLayoutStore(defaults: d).layout == mine)
            #expect(written.contains(#""version":1"#) && written.contains(#""bar:large""#), "\(written)")
        }
        // Nothing stored: the default, nothing written.
        let empty = MemoryDefaults()
        #expect(TransportLayoutStore(defaults: empty).layout == .standard)
        #expect(empty.string(forKey: key) == nil)
    }

    @Test func itemsTextParses() throws {
        func message(_ text: String) -> String? {
            if case .failure(let e) = TransportLayout.parse(text) { return e.message }
            return nil
        }
        let l = try TransportLayout.parse("cluster,loop,bar:large").get()
        #expect(l.items == [.builtIn(.cluster, nil), .builtIn(.loop, nil), .builtIn(.bar, .large)])
        #expect(l.text == "cluster,loop,bar:large")
        #expect(try TransportLayout.parse(TransportLayout.standard.text).get() == .standard)
        #expect(try TransportLayout.parse(" cluster , space,separator, space ").get().items.count == 4)
        #expect(message("loop") == "the bar needs cluster once")
        #expect(message("cluster,cluster") == "the bar needs cluster once")
        #expect(message("cluster,clik") == "unknown item clik")
        #expect(message("cluster,loop:large") == "loop has no size")
        #expect(message("cluster,bar:huge") == "bar has no size huge")
        #expect(message("cluster,loop,loop") == "loop is in the bar twice")
        #expect(message("cluster,,loop") == "an empty item")
        #expect(message("") == "an empty item")
        #expect(message(Array(repeating: "space", count: 64).joined(separator: ",")) == "the bar needs cluster once")
        #expect(message((["cluster"] + Array(repeating: "space", count: 64)).joined(separator: ",")) == "at most 64 items")
        #expect(message(String(repeating: "x", count: 5000))?.count ?? 0 < 80, "a hostile token is quoted short")
        #expect(message("cluster,Loop") == "unknown item Loop")
        #expect(message("cluster,replaceOverdub") == "unknown item replaceOverdub")
    }

    private func window(_ v: NSView, width: CGFloat, height: CGFloat) -> NSWindow {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: width, height: height), styleMask: [.titled], backing: .buffered,
            defer: true)
        w.isReleasedWhenClosed = false
        w.contentView = v
        return w
    }

    private func mixerRow(_ m: DocumentModel) -> TransportRowView {
        TransportRowView(document: m, layout: .standard, documentWindow: false) { item in
            appWidths[item.id].map { StandIn($0) }
        }
    }

    /// Review Focus 5.
    @Test func theMixerWindowRowAt300KeepsEverything() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let row = mixerRow(m)
        #expect(!row.slots.contains { $0.item.id.documentOnly }, "the mixer window has no document-only item")
        let w = window(row, width: 300, height: row.fittingHeight)
        defer { w.close() }
        row.layoutSubtreeIfNeeded()
        #expect(row.bounds.width == 300)
        let visible = row.slots.filter { !$0.view.isHidden }
        let mid = row.chevron.frame.midY
        for s in visible + [TransportRowView.Slot(item: .builtIn(.cluster, nil), view: row.chevron)] {
            #expect(abs(s.view.frame.midY - mid) < 0.5, "\(s.item.id) \(s.view.frame)")
            #expect(s.view.frame.maxX <= 300, "\(s.item.id) \(s.view.frame)")
            #expect(s.view.frame.minX >= 0, "\(s.item.id) \(s.view.frame)")
        }
        #expect(visible.contains { $0.item.id == .cluster }, "the cluster shows")
        #expect(!row.chevron.isHidden, "» shows")
        #expect(row.chevron.accessibilityLabel() == "More Transport Items")
        let folded = row.foldedItems
        #expect(!folded.isEmpty)
        let order = row.slots.map(\.item)
        let at = folded.compactMap { order.firstIndex(of: $0) }
        #expect(at == at.sorted(), "in bar order")
        let titles = row.overflowMenu().items.map(\.title)
        #expect(titles.count == folded.count, "\(titles)")
        for (title, item) in zip(titles, folded) {
            #expect(title.hasPrefix(item.id.title), "\(title) is \(item.id)")
        }
        #expect(!folded.contains { $0.id.documentOnly })
        let shownOrFolded = Set(row.shownItems.map(\.id)).union(folded.map(\.id))
        // Widening brings them back, in order, none lost on the way.
        let content = Set(row.slots.map(\.item.id).filter { ![.space, .separator, .summary].contains($0) })
        // The real readouts and fields need about 1,790 pt in the mixer
        // window (the brief's 1,600 assumed narrower ones): at 1,600 only
        // the tail is folded; by 2,000 nothing is.
        let inBar = order.map(\.id).filter { content.contains($0) }
        var lastFolded = Int.max
        for width in stride(from: CGFloat(300), through: 2000, by: 50) {
            w.setContentSize(NSSize(width: width, height: row.fittingHeight))
            row.layoutSubtreeIfNeeded()
            let ids = Set(row.shownItems.map(\.id)).union(row.foldedItems.map(\.id))
            #expect(ids == content, "\(width): \(ids.symmetricDifference(content))")
            #expect(row.slots.filter { !$0.view.isHidden }.allSatisfy { $0.view.frame.maxX <= width }, "\(width)")
            #expect(row.foldedItems.map(\.id) == Array(inBar.suffix(row.foldedItems.count)), "\(width): the tail folds")
            #expect(row.foldedItems.count <= lastFolded, "\(width): widening only brings items back")
            lastFolded = row.foldedItems.count
            if width == 1600 { #expect(row.foldedItems.count <= 2, "1600: \(row.foldedItems)") }
        }
        #expect(shownOrFolded == content)
        #expect(row.foldedItems.isEmpty, "at 2000 nothing folds")
        #expect(row.chevron.isHidden, "and » is hidden")
        #expect(row.shownItems.map(\.id) == inBar, "in bar order")
    }

    /// The fields fold as views, never remade; a popover lends the view
    /// and gives it back.
    @Test func foldedFieldsAreHiddenNotRemade() throws {
        let m = try DocumentModel(session: demoFake())
        let row = mixerRow(m)
        let w = window(row, width: 1600, height: row.fittingHeight)
        defer { w.close() }
        row.layoutSubtreeIfNeeded()
        let fields = row.slots.compactMap { $0.view as? TransportFieldsView }
        #expect(fields.map(\.shownFields) == [[.position], [.editIn, .editOut], [.punchIn, .punchOut]])
        w.setContentSize(NSSize(width: 300, height: row.fittingHeight))
        row.layoutSubtreeIfNeeded()
        let hidden = fields.allSatisfy { $0.isHidden }
        #expect(hidden)
        let same = row.slots.compactMap { $0.view as? TransportFieldsView }.elementsEqual(fields) { $0 === $1 }
        #expect(same, "the same views")
        #expect(!fields[0].focus(.position), "a folded field does not take the P key")
        // Tab goes on from Edit Out to Punch In across the items.
        #expect(fields[1].fields[.editOut]?.nextKeyView === fields[2].fields[.punchIn])
        #expect(fields[2].fields[.punchOut]?.nextKeyView === fields[0].fields[.position])
        // The P, E and U keys find the view that shows their field.
        #expect(TransportFieldsView.find(in: row, showing: .punchIn) === fields[2])
        // A new layout keeps the views of the items it still has.
        row.setLayout(try TransportLayout.parse("cluster,punch_fields,loop").get())
        #expect(row.slots.compactMap { $0.view as? TransportFieldsView }.first === fields[2])
        #expect(fields[0].superview == nil)
    }

    @Test func largeReadoutsUseTheLargeFont() throws {
        let m = try DocumentModel(session: demoFake())
        for t in [Theme.classicLight, .studioDark] {
            let large = TransportReadoutsView(document: m, boxes: [.bar], size: .large)
            let small = TransportReadoutsView(document: m, boxes: [.time], size: .small)
            large.theme = t
            small.theme = t
            let title = t.digitsFont(t.type.title, weight: t.type.title.weight.appKit)
            #expect(large.valueField(.bar)?.font == title, "\(t.id)")
            #expect(large.valueField(.time) == nil, "one box")
            let s = try #require(small.valueField(.time)?.font)
            #expect(s.pointSize < title.pointSize, "\(t.id): small is smaller")
            #expect(small.intrinsicContentSize.width < TransportReadoutsView(document: m, boxes: [.time]).intrinsicContentSize.width)
        }
        // The row makes them at the item's size.
        let row = TransportRowView(
            document: m, layout: try TransportLayout.parse("cluster,bar:large,time:small,tempo").get(),
            documentWindow: true
        ) { _ in nil }
        let readouts = row.slots.compactMap { $0.view as? TransportReadoutsView }
        #expect(readouts.map(\.size) == [.large, .small, .large])
        #expect(readouts.map(\.shownBoxes) == [[.bar], [.time], [.tempo]])
    }

    @Test func theClickItemSetsLevelAndMode() throws {
        let f = demoFake()
        f.state.click = FakeSession.defaultClick
        let m = try DocumentModel(session: f)
        m.poll()
        func checked(_ menu: TransportMenuEntry?) -> [String] {
            (menu?.entries ?? []).filter { $0.isOn == true }.map(\.title)
        }
        func level(_ entries: [TransportMenuEntry]) -> TransportMenuEntry? { entries.first { $0.title == "Level" } }
        func mode(_ entries: [TransportMenuEntry]) -> TransportMenuEntry? { entries.first { $0.title == "Mode" } }
        let menu = TransportItemEntries.clickMenu(m)
        #expect(menu.map(\.title) == ["Level", "Mode"])
        #expect(level(menu)?.entries.map(\.title) == ["0 dB", "−3 dB", "−6 dB", "−12 dB", "−18 dB", "−24 dB", "−36 dB", "−48 dB", "−60 dB"])
        #expect(checked(level(menu)) == ["−6 dB"], "a new song's level")
        #expect(checked(mode(menu)) == ["Play and Record"])
        #expect(level(menu)?.entry("−12 dB")?.choose() == true)
        #expect(f.clickRuns.last?.id == "transport.set_click_level")
        #expect(f.clickRuns.last?.args == [ControlArg(name: "level_db", value: .text(value: "-12"))])
        #expect(checked(level(TransportItemEntries.clickMenu(m))) == ["−12 dB"], "the check follows")
        #expect(mode(TransportItemEntries.clickMenu(m))?.entry("Record Only")?.choose() == true)
        #expect(f.clickRuns.last?.id == "transport.set_click_mode")
        #expect(f.clickRuns.last?.args == [ControlArg(name: "mode", value: .text(value: "record_only"))])
        #expect(checked(mode(TransportItemEntries.clickMenu(m))) == ["Record Only"])
        // The item's toggle.
        TransportItemEntries.toggleClick(m)
        #expect(f.clickRuns.last?.id == "transport.click" && f.clickRuns.last?.args.isEmpty == true)
        #expect(TransportItemEntries.clickIsOn(m))
        // Folded into »: the same check and submenus.
        let folded = try #require(TransportItemEntries.overflow(.builtIn(.click, nil), m, documentWindow: false))
        #expect(folded.title == "Click" && folded.isOn == true)
        #expect(folded.entries.map(\.title) == ["Click", "Level", "Mode"])
        #expect(folded.entry("Click")?.isOn == true)
        #expect(checked(level(folded.entries)) == ["−12 dB"])
        #expect(checked(mode(folded.entries)) == ["Record Only"])
        let ns = TransportMenus.menu([folded]) { _ in }
        let item = try #require(ns.items.first)
        #expect(item.title == "Click" && item.state == .on)
        #expect(item.submenu?.items.map(\.title) == ["Click", "Level", "Mode"])
        #expect(item.submenu?.items[1].submenu?.items.first { $0.state == .on }?.title == "−12 dB")
        // Choosing in the NSMenu runs the same command.
        let minus18 = try #require(item.submenu?.items[1].submenu?.items.first { $0.title == "−18 dB" })
        _ = (minus18.target as? NSObject)?.perform(minus18.action, with: minus18)
        #expect(f.clickRuns.last?.args == [ControlArg(name: "level_db", value: .text(value: "-18"))])
        // No settings from the session yet: off, and nothing to choose.
        f.state.click = nil
        m.poll()
        let none = TransportItemEntries.clickMenu(m)
        #expect(none.allSatisfy { !$0.enabled })
        #expect(level(none)?.entries.allSatisfy { !$0.enabled && $0.isOn == false } == true)
        #expect(!TransportItemEntries.clickIsOn(m))
        let runs = f.clickRuns.count
        #expect(level(none)?.entry("0 dB")?.choose() == false)
        #expect(f.clickRuns.count == runs)
    }

    @Test func setTransportBarOverTheChannel() throws {
        let store = TransportLayoutStore(defaults: MemoryDefaults())
        guard case .done = TransportLayoutCommands.run("ui.set_transport_bar", items: "cluster,loop", store: store)
        else {
            Issue.record("set failed")
            return
        }
        #expect(store.layout.items == [.builtIn(.cluster, nil), .builtIn(.loop, nil)])
        for bad in ["cluster,clik", "loop", String(repeating: "cluster,", count: 70)] {
            guard case .failed(let e) = TransportLayoutCommands.run("ui.set_transport_bar", items: bad, store: store)
            else {
                Issue.record("\(bad) was taken")
                continue
            }
            #expect(e.code == .invalidParams, "\(bad)")
            #expect(e.message.hasPrefix("ui.set_transport_bar: "), "\(e.message)")
            #expect(store.layout.items.count == 2, "unchanged")
        }
        if case .failed(let e) = TransportLayoutCommands.run("ui.set_transport_bar", items: "cluster,clik", store: store) {
            #expect(e.message.hasSuffix("unknown item clik"))
        }
        if case .failed(let e) = TransportLayoutCommands.run("ui.set_transport_bar", items: nil, store: store) {
            #expect(e.code == .invalidParams)
        } else {
            Issue.record("no items was taken")
        }
        guard case .done = TransportLayoutCommands.run("ui.reset_transport_bar", items: nil, store: store) else {
            Issue.record("reset failed")
            return
        }
        #expect(store.layout == .standard)
        // Through the channel's dispatch: no document needed.
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        #expect(ControlCommands.handled.isSuperset(of: TransportLayoutCommands.ids))
        let args = [ControlArg(name: "items", value: .text(value: "cluster,click,separator,bar:small"))]
        guard case .done = ControlCommands.run("ui.set_transport_bar", args: args, doc: nil, store: s, host: FakeControlHost())
        else {
            Issue.record("the channel's set failed")
            return
        }
        #expect(s.transportLayout.layout.text == "cluster,click,separator,bar:small")
        guard case .done = ControlCommands.run("ui.reset_transport_bar", args: [], doc: nil, store: s, host: FakeControlHost())
        else {
            Issue.record("the channel's reset failed")
            return
        }
        #expect(s.transportLayout.layout == .standard)
    }
}
