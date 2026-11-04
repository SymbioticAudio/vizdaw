import AppKit
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The audio editor's canvas (plan 7c Task 6) on `audioEditorDocument()`'s
/// region 71, framed 644 × 218 so the wave rect is 600 × 200, in a window
/// that is never shown (AppKit keeps `needsDisplay` and the first
/// responder only for a view in a window).
@MainActor
private func audioCanvas(select: [UInt64] = [71]) -> (FakeSession, DocumentModel, AudioEditorCanvas, NSWindow) {
    let (f, m) = audioEditorDocument()
    selectEvents(select, in: m)
    let c = AudioEditorCanvas(document: m)
    c.frame = NSRect(x: 0, y: 0, width: 644, height: 218)
    let w = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 644, height: 218), styleMask: [.titled], backing: .buffered, defer: true)
    w.isReleasedWhenClosed = false
    w.contentView = c
    m.audioEditor.sync(width: 600)
    f.calls = []
    return (f, m, c, w)
}

/// The centre of `h`'s drawn rect for the edited region.
@MainActor
private func centre(_ h: AudioEditorHandle, _ m: DocumentModel) throws -> CGPoint {
    let r = try #require(m.editedRegion).row
    let wave = AudioEditorGeometry.waveRect(in: CGRect(x: 0, y: 0, width: 644, height: 218))
    let rect = try #require(AudioEditorGeometry.drawn(r, axis: m.audioEditor.axis, in: wave).first { $0.0 == h }).1
    return CGPoint(x: rect.midX, y: rect.midY)
}

@MainActor
@Suite struct AudioEditorViewTests {
    @Test func theCanvasExposesItsHandles() async throws {
        let (f, m, c, w) = audioCanvas()
        defer { w.close() }
        #expect(c.accessibilityLabel() == "Audio Editor")
        let kids = (c.accessibilityChildren() ?? []).compactMap { $0 as? NSAccessibilityElement }
        #expect(
            kids.map { $0.accessibilityLabel() ?? "" }
                == ["Fade In Handle", "Fade Out Handle", "Start Handle", "End Handle", "Gain Handle"])
        for k in kids {
            #expect(c.bounds.contains(k.accessibilityFrameInParentSpace()), "\(k.accessibilityLabel() ?? "")")
        }
        // The waveform and the envelope ask for the same in-file span.
        c.display()
        let o = try #require(f.overviewCalls.last)
        let e = try #require(f.envelopeCalls.last)
        #expect(o.media == 5 && e.event == 71)
        #expect(o.from == e.from && o.to == e.to && o.columns == e.columns)
        // Another region is fitted by the observation refresh alone.
        selectEvents([72], in: m)
        await settle()
        #expect(m.audioEditor.shownRegion == 72)
        selectEvents([], in: m)
        #expect((c.accessibilityChildren() ?? []).isEmpty)
    }

    @Test func aHandleDragIsOneGesture() throws {
        let (f, m, c, w) = audioCanvas()
        defer { w.close() }
        let p = try centre(.gain, m)
        c.press(p, clicks: 1, modifiers: [])
        for step in 1...4 { c.drag(CGPoint(x: p.x, y: p.y - CGFloat(step * 5)), modifiers: []) }
        c.release(CGPoint(x: p.x, y: p.y - 20), modifiers: [])
        #expect(f.calls.filter { $0.hasPrefix("begin") } == ["begin Region Gain"])
        #expect(f.gestureEdits.last == .audio(edit: .setGain(event: 71, gainDb: 5.0)))
        #expect(f.calls.filter { $0 == "end" }.count == 1)
        #expect(m.editor.gesture == nil)
    }

    @Test func aTrimDragPastTheFileStartClamps() throws {
        let (f, m, c, w) = audioCanvas()
        defer { w.close() }
        let p = try centre(.start, m)
        c.press(p, clicks: 1, modifiers: [])
        c.drag(CGPoint(x: p.x - 10, y: p.y), modifiers: [])
        c.drag(CGPoint(x: p.x - 10_000, y: p.y), modifiers: [])
        c.release(CGPoint(x: p.x - 10_000, y: p.y), modifiers: [])
        #expect(f.calls.first == "begin Trim Start")
        #expect(f.gestureEdits.last == .audio(edit: .trim(event: 71, frontFrames: -24_000, backFrames: 0)))
    }

    @Test func anEscapeCancelsTheDrag() throws {
        let (f, m, c, w) = audioCanvas()
        defer { w.close() }
        // With nothing to cancel, Escape goes on up the chain (review M4).
        let after = KeyRecorder()
        c.nextResponder = after
        c.keyDown(with: try escape(in: w))
        #expect(after.keys == [53])
        let before = try #require(m.editedRegion).row.fadeIn
        let p = try centre(.fadeIn, m)
        c.press(p, clicks: 1, modifiers: [])
        c.drag(CGPoint(x: p.x + 40, y: p.y), modifiers: [])
        #expect(f.calls.first == "begin Fade In")
        c.keyDown(with: try escape(in: w))
        #expect(after.keys == [53], "a drag's Escape is the canvas's own")
        c.release(CGPoint(x: p.x + 40, y: p.y), modifiers: [])
        #expect(f.calls.filter { $0 == "abort" }.count == 1)
        #expect(!f.calls.contains("end"))
        #expect(m.editor.gesture == nil)
        #expect(m.editedRegion?.row.fadeIn == before, "the start state is back (review M7)")
    }

    @Test func theCanvasRedrawsWhenItsThemeChanges() {
        let (_, _, c, w) = audioCanvas()
        defer { w.close() }
        // Off screen AppKit reads `needsDisplay` back as true even after it
        // is cleared, so the requests are counted too.
        let before = c.displayRequests
        c.theme = c.theme == .console ? .studioDark : .console
        #expect(c.needsDisplay)
        #expect(c.displayRequests > before)
        let after = c.displayRequests
        let same = c.theme
        c.theme = same
        #expect(c.displayRequests == after, "the same theme redraws nothing")
    }
}

/// Records the key codes that reach it.
private final class KeyRecorder: NSResponder {
    var keys: [UInt16] = []
    override func keyDown(with event: NSEvent) { keys.append(event.keyCode) }
}

@MainActor
private func escape(in w: NSWindow) throws -> NSEvent {
    try #require(
        NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: w.windowNumber,
            context: nil, characters: "\u{1B}", charactersIgnoringModifiers: "\u{1B}", isARepeat: false,
            keyCode: 53))
}
