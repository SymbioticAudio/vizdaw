import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// `region.open_external` and `region.reload_external`, the menu items,
/// the keys and the editor preference (plan 5f-3 Task 5; Rulings 5, 7;
/// pre-flight I6, I7, I10, I11, M12; owner answers 2026-11-03).
@MainActor
@Suite struct ExternalCommandsTests {
    let host = FakeControlHost()
    let opener = FakeOpener()
    let watching = FakeWatching()
    let probe = FakeProbe()
    let clock = FakeClock()
    let path = "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav"

    /// A store with one demo document, given fake tools; jobs stay running.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        f.jobStartState = .running
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        s.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        probe.stamps[path] = FileStamp(size: 1000, modified: clock.t)
        return (s, id, m, f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: s.model(doc).map { (doc, $0) }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID) -> ControlOutcome { run(id, [], s, doc) }

    func app(_ path: String) -> [ControlArg] { [ControlArg(name: "app", value: .text(value: path))] }

    func select(_ m: DocumentModel, _ events: [UInt64]) {
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: events))
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    /// A temporary folder, removed by the caller.
    func folder() throws -> URL {
        let d = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-ext-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    @Test func externalCommandsCheckTheirArguments() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        for events in [[], [5, 6]] as [[UInt64]] {
            select(m, events)
            for c in [RegionCommands.openExternal, RegionCommands.reloadExternal] {
                let e = try #require(failure(run(c, s, id)))
                #expect(e.code == .unavailable && e.message == "Select one audio region first.", "\(c) \(events)")
            }
        }
        // A MIDI note: the core's refusal.
        select(m, [9])
        f.externalError = .Model(message: "Event 9 is not an audio region; select one audio region.")
        var e = try #require(failure(run(RegionCommands.openExternal, s, id)))
        #expect(e.code == .failed && e.message.contains("not an audio region"))
        f.externalError = nil
        select(m, [5])
        // No editor, no `app`, no stub: the Settings message, no panel.
        host.externalEditor = .none
        e = try #require(failure(run(RegionCommands.openExternal, s, id)))
        #expect(e.code == .unavailable && e.message == RegionCommands.noEditor)
        // An `app` that is missing or a plain file: refused before anything is written.
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("Plain.app").path
        #expect(FileManager.default.createFile(atPath: file, contents: Data()))
        host.externalEditor = .stub
        // A folder not ending in .app, and a relative path to a real
        // `.app` folder (final review M6, M7).
        let plain = dir.appendingPathComponent("Editor")
        try FileManager.default.createDirectory(at: plain, withIntermediateDirectories: true)
        let bundle = dir.appendingPathComponent("Editor.app")
        try FileManager.default.createDirectory(at: bundle, withIntermediateDirectories: true)
        let up = FileManager.default.currentDirectoryPath.split(separator: "/").map { _ in ".." }
        let relative = (up + [String(bundle.path.dropFirst())]).joined(separator: "/")
        #expect(FileManager.default.fileExists(atPath: relative), "the relative path names the bundle")
        for bad in ["/Applications/Nope-\(UUID().uuidString).app", file, plain.path, relative] {
            e = try #require(failure(run(RegionCommands.openExternal, app(bad), s, id)))
            #expect(e.code == .failed && e.message == "\(bad) is not an application.", "\(bad)")
        }
        // Reload of a region with no edit.
        e = try #require(failure(run(RegionCommands.reloadExternal, s, id)))
        #expect(e.code == .unavailable && e.message == ExternalEditModel.noEditMessage)
        #expect(f.externalOpened == [9], "only the note reached the core")
        #expect(opener.opened.isEmpty && watching.started.isEmpty && f.externalApplies.isEmpty)
    }

    @Test func openAnswersWithTheCopysPath() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        #expect(status(run(RegionCommands.openExternal, s, id)) == "editing \(path)")
        #expect(f.externalOpened == [5])
        #expect(opener.opened == [FakeOpener.Opened(file: URL(fileURLWithPath: path), app: nil)])
        #expect(m.external.watched == [5])
        // `app` wins over the preference; a folder ending in .app passes.
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let editor = dir.appendingPathComponent("Editor.app")
        try FileManager.default.createDirectory(at: editor, withIntermediateDirectories: true)
        host.externalEditor = .app("/Applications/Other.app")
        #expect(status(run(RegionCommands.openExternal, app(editor.path), s, id)) == "editing \(path)")
        #expect(opener.opened.last?.app == URL(fileURLWithPath: editor.path))
    }

    @Test func reloadWithNothingChangedAnswersUnchanged() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        _ = run(RegionCommands.openExternal, s, id)
        #expect(status(run(RegionCommands.reloadExternal, s, id)) == "unchanged")
        #expect(f.externalApplies.isEmpty)
    }

    @Test func reloadWithAChangeIsAJob() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        _ = run(RegionCommands.openExternal, s, id)
        probe.stamps[path] = FileStamp(size: 2000, modified: clock.t)
        guard case .job(let command, let doc, let job) = run(RegionCommands.reloadExternal, s, id) else {
            Issue.record("Reload did not start a job")
            return
        }
        #expect(command == RegionCommands.reloadExternal && doc == id && job == 5001)
        #expect(f.externalApplies == [FakeSession.ExternalApply(event: 5, path: path)])
        #expect(m.jobs.isEmpty, "the channel answers this job: no banner, no second report")
        let e = try #require(failure(run(RegionCommands.reloadExternal, s, id)))
        #expect(e.code == .busy && e.message == ExternalEditModel.importingMessage)
    }

    @Test func theMenuItemsAreEnabledForOneRegion() throws {
        let (_, _, m, f) = try demo()
        for c in RegionCommands.external {
            select(m, [])
            #expect(!RegionCommands.enabled(c, m))
            select(m, [5])
            #expect(RegionCommands.enabled(c, m))
            select(m, [5, 6])
            #expect(!RegionCommands.enabled(c, m))
            // Never an edit (pre-flight I6).
            select(m, [5])
            if case .unavailable = RegionCommands.run(c, m) {} else { Issue.record("\(c) ran as an edit") }
        }
        #expect(f.edits.isEmpty && f.externalOpened.isEmpty)
        select(m, [5, 6])
        #expect(RegionCommands.enabled(RegionCommands.removeFades, m), "the others take several regions")
    }

    @Test func aKeyWithNoEditorShowsTheSettingsMessage() throws {
        let (s, id, m, _) = try demo()
        select(m, [5])
        host.externalEditor = .none
        let o = run(RegionCommands.openExternal, s, id)
        #expect(KeyDispatch.after(o, doc: m) == .handled, "used up: passed on, it would reach the chooser")
        #expect(m.editor.status == RegionCommands.noEditor)
        #expect(opener.opened.isEmpty)
        select(m, [])
        #expect(KeyDispatch.after(run(RegionCommands.removeFades, s, id), doc: m) == .pass, "others still pass")
        // The chord the menu item shows, too: never left to the menu, whose
        // item opens the chooser (fold check I1, owner answer 2026-11-03).
        let keys = KeyBindings(defaults: MemoryDefaults())
        try keys.bind(RegionCommands.openExternal, keys: ["⌃⌥⌘E"], replace: true)
        #expect(
            KeyRouter.route(
                keyCode: 14, modifiers: ModifiersRow(shift: false, control: true, option: true, command: true),
                typing: false, stepEntry: false, menus: true, keys: keys, sequences: [])
                == .command(RegionCommands.openExternal))
    }

    /// Final review M8: the Open key with an editor set opens the copy
    /// and is used up.
    @Test func theOpenKeyWithAnEditorSetOpensTheCopy() throws {
        let (s, id, m, f) = try demo()
        select(m, [5])
        host.externalEditor = .stub
        let keys = KeyBindings(defaults: MemoryDefaults())
        try keys.bind(RegionCommands.openExternal, keys: ["⌃⌥⌘E"], replace: true)
        let r = KeyRouter.route(
            keyCode: 14, modifiers: ModifiersRow(shift: false, control: true, option: true, command: true),
            typing: false, stepEntry: false, menus: true, keys: keys, sequences: [])
        guard case .command(let c) = r else {
            Issue.record("\(r)")
            return
        }
        #expect(KeyDispatch.command(c, doc: (id, m), store: s, host: host) == .handled)
        #expect(opener.opened.count == 1 && f.externalOpened == [5] && m.external.watched == [5])
        #expect(m.editor.status == "editing \(path)")
    }

    /// Final review M9: a key-bound or surface Reload reports as the menu
    /// item does: an import shows its banner (so a failure gets its
    /// alert), and nothing changed says so in words.
    @Test func aKeyOrSurfaceReloadReportsAsTheMenuDoes() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [])
        #expect(
            KeyDispatch.command(RegionCommands.reloadExternal, doc: (id, m), store: s, host: host) == .pass,
            "no region: passed on, as before")
        select(m, [5])
        _ = run(RegionCommands.openExternal, s, id)
        #expect(KeyDispatch.command(RegionCommands.reloadExternal, doc: (id, m), store: s, host: host) == .handled)
        #expect(m.editor.status == "The external edit is unchanged.")
        probe.stamps[path] = FileStamp(size: 2000, modified: clock.t)
        #expect(KeyDispatch.command(RegionCommands.reloadExternal, doc: (id, m), store: s, host: host) == .handled)
        #expect(m.jobs.map(\.kind) == [.externalEdit], "the import shows its banner")
        #expect(f.externalApplies == [FakeSession.ExternalApply(event: 5, path: path)])
    }

    @Test func theMenuItemsReportInTheWindow() throws {
        let (_, _, m, f) = try demo()
        select(m, [5])
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.editor.status == ExternalEditModel.noEditMessage)
        RegionCommands.runMenu(RegionCommands.openExternal, m, editor: .stub)
        #expect(f.externalOpened == [5] && m.external.watched == [5] && m.alert == nil)
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.editor.status == "The external edit is unchanged.")
        probe.stamps[path] = FileStamp(size: 2000, modified: clock.t)
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.jobs.map(\.kind) == [.externalEdit], "the menu's import shows its banner")
        f.externalError = .Io(message: "Media/Edits is a file, not a folder; move it away and try again.")
        RegionCommands.runMenu(RegionCommands.openExternal, m, editor: .stub)
        #expect(m.alert?.title == ExternalEditModel.openFailedTitle)
    }

    @Test func theEditorPreferenceIsKept() {
        let d = MemoryDefaults()
        let p = EditingPreferences(defaults: d)
        #expect(p.externalEditor == nil)
        p.externalEditor = "/Applications/Editor.app"
        #expect(d.string(forKey: EditingPreferences.externalEditorKey) == "/Applications/Editor.app")
        #expect(EditingPreferences(defaults: d).externalEditor == "/Applications/Editor.app")
        p.externalEditor = nil
        #expect(EditingPreferences(defaults: d).externalEditor == nil)
    }
}
