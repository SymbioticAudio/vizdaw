import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Scripted answers.
@MainActor
final class FakeQuestions: RecoveryQuestions {
    var crash: CrashChoice = .normally
    var restore = false
    var asked: [String] = []

    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        asked.append("crash \(project) \(crashed) \(loading.map(\.name))")
        return crash
    }

    func restoreAutosave(project: String) -> Bool {
        asked.append("restore \(project)")
        return restore
    }
}

@MainActor
@Suite struct RecoveryTests {
    let diva = PluginRefRow(format: .au, id: "aumu/Diva/UHfX", name: "Diva", vendor: "u-he", version: "1")

    func store(
        _ info: RecoveryInfo, _ q: FakeQuestions, shift: Bool = false
    ) -> (DocumentStore, () -> [OpenOptions], () -> [[PluginRefRow]]) {
        var opened: [OpenOptions] = []
        var disabled: [[PluginRefRow]] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.recovery = Recovery(
            check: { _ in info },
            openWith: { _, o in
                opened.append(o)
                return demoFake()
            },
            disable: { disabled.append($0) },
            shiftHeld: { shift },
            questions: q)
        return (s, { opened }, { disabled })
    }

    @Test func aCleanProjectOpensWithoutQuestions() {
        let q = FakeQuestions()
        let (s, opened, _) = store(RecoveryInfo(crashed: false, loading: [], autosave: nil), q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(q.asked.isEmpty)
        #expect(opened() == [OpenOptions(pluginsOff: false, autosave: nil)])
    }

    @Test func afterACrashPluginsOffOrCancel() {
        let q = FakeQuestions()
        q.crash = .pluginsOff
        let info = RecoveryInfo(crashed: true, loading: [diva], autosave: nil)
        let (s, opened, _) = store(info, q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(q.asked == ["crash Song true [\"Diva\"]"])
        #expect(opened() == [OpenOptions(pluginsOff: true, autosave: nil)])
        q.crash = .cancel
        #expect(s.open(path: "/p/Song.vizdaw") == nil)
        #expect(opened().count == 1)
        #expect(s.alert == nil)
    }

    @Test func disablingTheCrashingPluginThenOpening() {
        let q = FakeQuestions()
        q.crash = .disable
        let (s, opened, disabled) = store(RecoveryInfo(crashed: true, loading: [diva], autosave: nil), q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(disabled() == [[diva]])
        #expect(opened() == [OpenOptions(pluginsOff: false, autosave: nil)])
    }

    @Test func shiftAsksEvenWithoutACrash() {
        let q = FakeQuestions()
        q.crash = .pluginsOff
        let (s, opened, _) = store(RecoveryInfo(crashed: false, loading: [], autosave: nil), q, shift: true)
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(q.asked == ["crash Song false []"])
        #expect(opened().first?.pluginsOff == true)
    }

    @Test func aNewerAutosaveIsOfferedAndRestored() {
        let q = FakeQuestions()
        q.restore = true
        let info = RecoveryInfo(crashed: false, loading: [], autosave: "/p/Song.vizdaw/Autosave/project-1.json")
        let (s, opened, _) = store(info, q)
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(q.asked == ["restore Song"])
        #expect(opened().first?.autosave == "/p/Song.vizdaw/Autosave/project-1.json")
        q.restore = false
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(opened().last?.autosave == nil)
    }

    @Test func closingADocumentClosesItsSessionCleanly() throws {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { f }, makeDemo: { f }, open: { _ in f })
        _ = try #require(s.newEmpty())
        _ = try #require(s.newDemo())
        s.closeAll()
        #expect(f.closeCalls == 2)
        #expect(s.documents.isEmpty)
    }
}
