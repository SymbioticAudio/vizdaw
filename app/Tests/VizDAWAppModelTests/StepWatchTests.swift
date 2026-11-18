import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Steps, undos and redos from snapshots (plan 8f Ruling 4).
@MainActor
@Suite struct StepWatchTests {
    @Test func stepsUndosAndRedosAreTold() {
        var w = StepWatch()
        #expect(w.observe(step: 4, undoLabel: "Add Track") == nil)
        #expect(w.observe(step: 5, undoLabel: "Rename Track") == .newStep(label: "Rename Track"))
        #expect(w.observe(step: 4, undoLabel: "Add Track") == .undo)
        #expect(w.observe(step: 5, undoLabel: "Rename Track") == .redo)
        #expect(w.observe(step: 6, undoLabel: "Delete Track") == .newStep(label: "Delete Track"))
        #expect(w.observe(step: 6, undoLabel: "Delete Track") == nil)
        // A fall of several steps at once is one undo, and climbing back
        // below the highest is a redo.
        #expect(w.observe(step: 2, undoLabel: nil) == .undo)
        #expect(w.observe(step: 4, undoLabel: "Add Track") == .redo)
        // A step with no undo label is still a step.
        #expect(w.observe(step: 7, undoLabel: nil) == .newStep(label: ""))
    }

    func snap(_ f: FakeSession, step: UInt64, label: String?) -> DocSnapshot {
        var s = f.snap
        s.version += 1
        s.step = step
        s.undoLabel = label
        return s
    }

    @Test func documentsForwardTheirSteps() throws {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        final class Heard { var events: [String] = [] }
        let heard = Heard()
        s.stepObserver = { id, e in heard.events.append("\(id.uuidString) \(e)") }
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))

        // The model tells its own observer, and the store tells its own
        // with the document's ID.
        final class Own { var events: [StepEvent] = [] }
        let own = Own()
        m.onStep = { [inner = m.onStep] e in
            own.events.append(e)
            inner?(e)
        }
        m.apply(snap(f, step: 1, label: "Rename Track"))
        m.apply(snap(f, step: 1, label: "Rename Track"))
        m.apply(snap(f, step: 0, label: nil))
        m.apply(snap(f, step: 1, label: "Rename Track"))
        #expect(own.events == [.newStep(label: "Rename Track"), .undo, .redo])
        #expect(heard.events == [
            "\(id.uuidString) newStep(label: \"Rename Track\")", "\(id.uuidString) undo",
            "\(id.uuidString) redo",
        ])

        // An observer set later reaches documents already open, and nil
        // stops it.
        let s2 = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id2 = try #require(s2.newDemo())
        let m2 = try #require(s2.model(id2))
        heard.events = []
        s2.stepObserver = { id, e in heard.events.append("\(id.uuidString) \(e)") }
        m2.apply(snap(f, step: 2, label: "Add Track"))
        #expect(heard.events == ["\(id2.uuidString) newStep(label: \"Add Track\")"])
        s2.stepObserver = nil
        m2.apply(snap(f, step: 3, label: "Add Track"))
        #expect(heard.events.count == 1)
    }
}
