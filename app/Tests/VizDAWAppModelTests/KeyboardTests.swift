import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
@Suite struct KeyboardTests {
    func board() throws -> (FakeSession, DocumentModel, PianoKeyboardView) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let k = PianoKeyboardView(model: m.pianoRoll(.pane))
        k.frame = NSRect(x: 0, y: 0, width: 64, height: 400)
        k.pitchAxis = PitchAxis(rowHeight: 10, offset: 600)
        return (f, m, k)
    }

    func y(_ k: PianoKeyboardView, _ p: UInt8) -> CGFloat { EditorCanvas.rulerHeight + k.pitchAxis.center(p) }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func onlyCsAreLabelled() {
        #expect(KeyboardLabels.label(60) == "C4")
        #expect(KeyboardLabels.label(0) == "C-1")
        #expect(KeyboardLabels.label(61) == nil)
    }

    @Test func aKeyClickAuditionsThroughTheEditLane() throws {
        let (f, m, k) = try board()
        defer { withExtendedLifetime(m) {} }
        k.press(atY: y(k, 60))
        k.release()
        #expect(log(f) == ["on 60", "off 60"])
        #expect(f.auditions.allSatisfy { $0.dest == 5 }, "Chords' destination")
    }

    @Test func draggingAcrossKeysPlaysEachInTurn() throws {
        let (f, m, k) = try board()
        defer { withExtendedLifetime(m) {} }
        k.press(atY: y(k, 60))
        k.dragTo(y: y(k, 60) + 2)
        k.dragTo(y: y(k, 62))
        k.release()
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
    }

    @Test func theRulerStripIsNotAKey() throws {
        let (f, m, k) = try board()
        defer { withExtendedLifetime(m) {} }
        k.press(atY: 5)
        k.release()
        #expect(f.auditions.isEmpty)
    }

    @Test func pianoRollModelsAreKeptPerSlot() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        let a = m.pianoRoll(.pane)
        #expect(m.pianoRoll(.pane) === a)
        let id = UUID()
        let w = m.pianoRoll(.window(id))
        #expect(w !== a)
        w.sound(60, velocity: 0x8000, dest: 5)
        m.dropPianoRoll(.window(id))
        #expect(f.auditions.last?.on == false, "a closing window silences its note")
        #expect(m.pianoRoll(.window(id)) !== w)
    }

    @Test func leavingTheWindowSilencesAHeldKey() throws {
        let (f, m, k) = try board()
        defer { withExtendedLifetime(m) {} }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 400), styleMask: [.titled], backing: .buffered, defer: true)
        w.isReleasedWhenClosed = false
        w.contentView?.addSubview(k)
        k.press(atY: y(k, 60))
        k.removeFromSuperview()
        #expect(log(f) == ["on 60", "off 60"])
    }
}
