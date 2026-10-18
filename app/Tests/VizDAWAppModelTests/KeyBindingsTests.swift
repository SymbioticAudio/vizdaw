import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

let noMods = ModifiersRow(shift: false, control: false, option: false, command: false)
let cmdMod = ModifiersRow(shift: false, control: false, option: false, command: true)
let optMod = ModifiersRow(shift: false, control: false, option: true, command: false)

@MainActor
@Suite struct KeyBindingsTests {
    @Test func bindingsPersistInThePreferences() throws {
        let d = MemoryDefaults()
        let k = KeyBindings(defaults: d)
        #expect(k.row("file.save")?.keys == ["⌘S"])
        #expect(k.command(keyCode: 49, modifiers: noMods, stepEntry: false) == "transport.play_stop")
        try k.bind("file.save", keys: ["⌘K"])
        #expect(k.row("file.save")?.keys == ["⌘K"] && k.row("file.save")?.menuKey == "k")
        k.reserveNumberRow = true
        let again = KeyBindings(defaults: d)
        #expect(again.row("file.save")?.keys == ["⌘K"])
        #expect(again.reserveNumberRow)
        // A damaged preference leaves the defaults.
        d.set("garbage", forKey: KeyBindings.key)
        #expect(KeyBindings(defaults: d).row("file.save")?.keys == ["⌘S"])
    }

    @Test func aTakenKeyIsRefusedUnlessReplaced() throws {
        let k = KeyBindings(defaults: MemoryDefaults())
        do {
            try k.bind("edit.undo", keys: ["⌘S"])
            Issue.record("bound a taken key")
        } catch {
            #expect(error as? VizError == .Model(message: "⌘S is the key of “Save”"))
        }
        try k.bind("edit.undo", keys: ["⌘S"], replace: true)
        #expect(k.row("file.save")?.keys == [])
        k.reset("file.save")
        #expect(k.row("file.save")?.keys == [], "⌘S stays with Undo")
        k.resetAll()
        #expect(k.row("file.save")?.keys == ["⌘S"] && k.row("edit.undo")?.keys == ["⌘Z"])
    }

    @Test func filesSaveLoadAndRefuseDamage() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vzk-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let k = KeyBindings(defaults: MemoryDefaults())
        try k.bind("file.save", keys: ["F5"])
        let path = dir.appendingPathComponent("mine.vizkeys").path
        try k.save(to: path)
        k.resetAll()
        try k.load(from: path)
        #expect(k.row("file.save")?.keys == ["F5"] && k.message == nil)
        let bad = dir.appendingPathComponent("bad.vizkeys")
        try Data("garbage".utf8).write(to: bad)
        #expect(throws: (any Error).self) { try k.load(from: bad.path) }
        #expect(k.row("file.save")?.keys == ["F5"], "unchanged")
        #expect(k.message?.isEmpty == false)
        try Data(#"{"vizdaw_keys": 1, "bindings": {"no.such": ["F1"]}}"#.utf8).write(to: bad)
        try k.load(from: bad.path)
        #expect(k.message == "Skipped: no command \"no.such\"")
        #expect(k.row("file.save")?.keys == ["⌘S"], "a loaded file replaces the bindings")
    }

    @Test func aSlotThatClashesNamesTheCommand() {
        let k = KeyBindings(defaults: MemoryDefaults())
        #expect(k.slotClash(KeySlotRow(key: "s", modifiers: 8)) == "Save")
        #expect(k.slotClash(KeySlotRow(key: "q", modifiers: 2)) == nil)
    }
}

@MainActor
@Suite struct KeyRouterTests {
    let keys = KeyBindings(defaults: MemoryDefaults())

    func route(
        _ code: UInt16, _ m: ModifiersRow = noMods, typing: Bool = false, step: Bool = false, menus: Bool = true,
        seqs: [SeqRow] = []
    ) -> KeyRoute {
        KeyRouter.route(
            keyCode: code, modifiers: m, typing: typing, stepEntry: step, menus: menus, keys: keys, sequences: seqs)
    }

    @Test func textFieldsKeepTheirPlainKeys() {
        #expect(route(49, typing: true) == .typing, "Space types a space")
        #expect(route(49) == .command("transport.play_stop"))
        #expect(route(1, cmdMod, typing: true, menus: false) == .command("file.save"), "⌘ keys still run")
    }

    @Test func menuKeysAreTheMenusOnTheKeyboardAndRunOnTheChannel() {
        #expect(route(1, cmdMod) == .pass, "the File menu saves, with its dialog")
        #expect(route(1, cmdMod, menus: false) == .command("file.save"))
        #expect(route(7) == .pass, "X is nothing")
    }

    @Test func keyboardSlotsNameTheirSequence() {
        var beat = seq(1, "Beat", topLevel: false)
        beat.keyboardSlot = KeySlotRow(key: "b", modifiers: 4)
        #expect(route(11, optMod, seqs: [beat]) == .slot(1))
        #expect(route(11, seqs: [beat]) == .pass, "without ⌥")
        #expect(route(11, optMod, typing: true, seqs: [beat]) == .typing)
    }

    @Test func aRefusedOrDialogCommandPassesTheKeyOn() throws {
        let m = try DocumentModel(session: demoFake())
        let ran = ControlResult.ran(command: "x", doc: nil, window: nil, undoLabel: nil, status: nil)
        #expect(KeyDispatch.after(.done(ran), doc: m) == .handled)
        #expect(KeyDispatch.after(.failed(.of(.unavailable, "no")), doc: m) == .pass)
        #expect(KeyDispatch.after(.failed(.dialog("Save", "path")), doc: m) == .pass)
        #expect(m.editor.status == nil)
        #expect(KeyDispatch.after(.failed(.of(.failed, "Refused.")), doc: m) == .handled)
        #expect(m.editor.status == "Refused.")
        let id = UUID()
        #expect(KeyDispatch.after(.opened(command: nil, doc: id), doc: m) == .opened(id))
    }

    @Test func onlyAMenuItemsFirstKeyIsLeftToTheMenu() throws {
        try keys.bind("file.save", keys: ["⌘S", "⌘J"])
        #expect(route(1, cmdMod) == .pass, "the key the menu shows")
        #expect(route(38, cmdMod) == .command("file.save"), "a second key the menu does not know")
    }
}

@Suite struct TransportMarksTests {
    @Test func marksSetWithinTheirLimits() {
        var t = TransportMarks()
        #expect(t.locators.count == 10 && t.locators.allSatisfy { $0 == nil })
        let set = t.setLocator(3, 61_440)
        #expect(set && t.locator(3) == 61_440)
        let refused = [t.setLocator(10, 0), t.setLocator(-1, 0), t.setLocator(1, 1 << 41)]
        #expect(refused == [false, false, false] && t.locator(10) == nil && t.locator(1) == nil)
        let cleared = t.setLocator(3, nil)
        #expect(cleared && t.locator(3) == nil)
        let empty = t.setPunch(in: 100, out: 100)
        #expect(!empty && t.punchOut == 4 * 61_440)
        let punched = t.setPunch(in: 0, out: 15_360)
        #expect(punched && t.punchOut == 15_360)
        var l = EditorLayout()
        l.transport = t
        #expect(EditorLayout.decode(l.encoded()).transport == t)
    }

    @Test func marksDecodeTolerantly() {
        let damaged = EditorLayout.decode(
            #"{"transport":{"locators":[1,null,"x"],"punchIn":50,"punchOut":10,"punch":"yes","overdub":true}}"#
        ).transport
        #expect(damaged.locators == Array(repeating: nil, count: 10), "an unreadable list: none set")
        #expect(damaged.punchIn == 0 && damaged.punchOut == 4 * 61_440, "out before in: the defaults")
        #expect(!damaged.punch && damaged.overdub)
        let odd = EditorLayout.decode(
            #"{"transport":{"locators":[1,null,9999999999999999,4,5,6,7,8,9,10,11,12],"punchIn":-5,"punchOut":7}}"#
        ).transport
        #expect(odd.locators == [1, nil, nil, 4, 5, 6, 7, 8, 9, 10], "past the limit: unset; ten kept")
        #expect(odd.punchIn == -5 && odd.punchOut == 7)
        #expect(EditorLayout.decode(#"{"snap":{"division":"half","triplet":false,"dotted":false,"enabled":true}}"#).transport == TransportMarks())
    }
}
