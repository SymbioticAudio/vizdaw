import AppKit
import CoreGraphics
import Foundation
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

/// 2 s at 48 kHz with the given fades and gain.
private func region(fadeIn: Int64 = 0, fadeOut: Int64 = 0, gain: Float = 0) -> EventRow {
    var r = Pencil.note(tick: 0, pitch: 0, length: 61_440)
    r.id = 4
    r.kind = .audioRegion
    r.media = 7
    r.frames = 96_000
    r.gainDb = gain
    r.fadeIn = FadeRow(length: fadeIn, shape: .linear)
    r.fadeOut = FadeRow(length: fadeOut, shape: .equalPower)
    return r
}

/// 160 points wide: 600 frames per point.
private let box = CGRect(x: 0, y: 10, width: 160, height: 40)

/// Spec §12.3: "handle hit-testing and drag maths for fades and gain".
@Suite struct RegionHandlesTests {
    @Test func handlesSitAtTheFadeEndsAndTheTopEdge() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fadeInX(r, in: box) == 20)
        #expect(RegionHandles.fadeOutX(r, in: box) == 120)
        #expect(RegionHandles.hit(CGPoint(x: 20, y: 12), row: r, in: box) == .fadeIn)
        #expect(RegionHandles.hit(CGPoint(x: 121, y: 13), row: r, in: box) == .fadeOut)
        #expect(RegionHandles.hit(CGPoint(x: 70, y: 11), row: r, in: box) == .gain)
        #expect(RegionHandles.hit(CGPoint(x: 70, y: 30), row: r, in: box) == nil, "the body is not a handle")
        // Too narrow for handles.
        let narrow = CGRect(x: 0, y: 10, width: 20, height: 40)
        #expect(RegionHandles.rects(r, in: narrow).isEmpty)
        #expect(RegionHandles.hit(CGPoint(x: 1, y: 11), row: r, in: narrow) == nil)
        // No frames: the handles sit at the edges, nothing divides by zero.
        var empty = r
        empty.frames = 0
        #expect(RegionHandles.fadeInX(empty, in: box).isFinite)
    }

    @Test func fadeDragsKeepBothFadesInsideTheRegion() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fades(r, .fadeIn, dx: 40, in: box) == (36_000, 24_000))
        #expect(RegionHandles.fades(r, .fadeIn, dx: -100, in: box) == (0, 24_000))
        #expect(RegionHandles.fades(r, .fadeIn, dx: 1_000, in: box) == (72_000, 24_000), "up to the fade out")
        #expect(RegionHandles.fades(r, .fadeOut, dx: -20, in: box) == (12_000, 36_000))
        #expect(RegionHandles.fades(r, .fadeOut, dx: 50, in: box) == (12_000, 0))
        #expect(RegionHandles.fadeCommand(r, .fadeIn, dx: 0, in: box) == nil)
        #expect(
            RegionHandles.fadeCommand(r, .fadeIn, dx: 40, in: box)
                == .audio(
                    edit: .setFades(
                        event: 4, fadeIn: FadeRow(length: 36_000, shape: .linear),
                        fadeOut: FadeRow(length: 24_000, shape: .equalPower))))
    }

    @Test func gainDragsInTenthsOfADecibelWithinTheRange() {
        let r = region()
        #expect(RegionHandles.gain(r, dy: -20, fine: false) == 5)
        #expect(RegionHandles.gain(r, dy: -20, fine: true) == 1)
        #expect(RegionHandles.gain(r, dy: 10, fine: false) == -2.5)
        #expect(RegionHandles.gain(r, dy: 1_000, fine: false) == -144)
        #expect(RegionHandles.gain(r, dy: -1_000, fine: false) == 24)
        #expect(RegionHandles.gain(r, dy: -1, fine: false) == 0.3)
        #expect(RegionHandles.gainCommand(r, dy: 0, fine: false) == nil)
        #expect(RegionHandles.gainCommand(r, dy: -20, fine: false) == .audio(edit: .setGain(event: 4, gainDb: 5)))
        #expect(RegionHandles.resetGain(r) == nil)
        #expect(RegionHandles.resetGain(region(gain: 3)) == .audio(edit: .setGain(event: 4, gainDb: 0)))
        #expect(RegionHandles.gainLabel(0) == "0.0 dB")
        #expect(RegionHandles.gainLabel(0.04) == "0.0 dB")
        #expect(RegionHandles.gainLabel(3) == "+3.0 dB")
        #expect(RegionHandles.gainLabel(-6.5) == "-6.5 dB")
        #expect(RegionHandles.label(.gain) == "Region Gain")
        #expect(RegionHandles.label(.fadeIn) == "Fade In")
    }

    /// Preflight M9: one drag target for both editors.
    @Test func aHandleDragsTargetDependsOnItsHandle() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(
            RegionHandles.target(r, .gain, rect: box, dx: 40, dy: -20, fine: false)
                == .audio(edit: .setGain(event: 4, gainDb: 5)))
        #expect(
            RegionHandles.target(r, .gain, rect: box, dx: 0, dy: -20, fine: true)
                == .audio(edit: .setGain(event: 4, gainDb: 1)))
        #expect(
            RegionHandles.target(r, .fadeIn, rect: box, dx: 40, dy: -20, fine: false)
                == RegionHandles.fadeCommand(r, .fadeIn, dx: 40, in: box))
        #expect(
            RegionHandles.target(r, .fadeOut, rect: box, dx: -20, dy: 7, fine: true)
                == RegionHandles.fadeCommand(r, .fadeOut, dx: -20, in: box))
        #expect(RegionHandles.target(r, .gain, rect: box, dx: 40, dy: 0, fine: false) == nil, "x does not move the gain")
        #expect(RegionHandles.target(r, .fadeIn, rect: box, dx: 0, dy: 30, fine: false) == nil, "y does not move a fade")
    }

    @Test func fadeShapesAreSetFromTheFadesArea() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fadeArea(CGPoint(x: 10, y: 30), row: r, in: box) == .fadeIn)
        #expect(RegionHandles.fadeArea(CGPoint(x: 140, y: 30), row: r, in: box) == .fadeOut)
        #expect(RegionHandles.fadeArea(CGPoint(x: 70, y: 30), row: r, in: box) == nil)
        #expect(RegionHandles.fadeArea(CGPoint(x: 10, y: 30), row: region(), in: box) == nil, "no fade, no menu")
        #expect(
            RegionHandles.shapeCommand(r, .fadeIn, .sCurve)
                == .audio(
                    edit: .setFades(
                        event: 4, fadeIn: FadeRow(length: 12_000, shape: .sCurve),
                        fadeOut: FadeRow(length: 24_000, shape: .equalPower))))
        #expect(RegionHandles.shapeCommand(r, .fadeOut, .equalPower) == nil, "already that shape")
        #expect(RegionHandles.shapeCommand(r, .gain, .linear) == nil)
        #expect(RegionHandles.shapes.map(\.1) == ["Linear", "Equal Power", "Exponential", "S-Curve"])
    }
}

/// Spec §12.3: "the overview drawing geometry".
@Suite struct WaveformGeometryTests {
    func rows(_ ch: UInt16, _ min: [Float], _ max: [Float], state: OverviewStateRow = .ready) -> OverviewRows {
        OverviewRows(state: state, channels: ch, columns: UInt32(min.count / Int(ch)), min: min, max: max)
    }

    @Test func barsSpanEachColumnsMinToMax() {
        let b = WaveformGeometry.bars(rows(1, [-1, -0.5, 0, 0.5], [1, 0.5, 0, 1]), in: CGRect(x: 0, y: 0, width: 40, height: 20))
        #expect(b.count == 4)
        #expect(b[0] == CGRect(x: 0, y: 0, width: 10, height: 20))
        #expect(b[1] == CGRect(x: 10, y: 5, width: 10, height: 10))
        #expect(b[2] == CGRect(x: 20, y: 10, width: 10, height: 0.5), "silence is a hairline")
        #expect(b[3] == CGRect(x: 30, y: 0, width: 10, height: 5))
    }

    @Test func channelsStackAndOversClip() {
        let b = WaveformGeometry.bars(rows(2, [-0.5, -3], [0.5, 2]), in: CGRect(x: 0, y: 0, width: 10, height: 40))
        #expect(b.count == 2)
        #expect(b[0] == CGRect(x: 0, y: 5, width: 10, height: 10), "left channel, top half")
        #expect(b[1] == CGRect(x: 0, y: 20, width: 10, height: 20), "right channel clipped to its half")
    }

    /// Review Focus 5.
    @Test func degenerateRowsDrawNothing() {
        let r = CGRect(x: 0, y: 0, width: 10, height: 10)
        #expect(WaveformGeometry.bars(rows(1, [], []), in: r).isEmpty)
        #expect(WaveformGeometry.bars(OverviewRows(state: .ready, channels: 1, columns: 4, min: [0], max: [0]), in: r).isEmpty)
        #expect(WaveformGeometry.bars(OverviewRows(state: .ready, channels: 0, columns: 1, min: [0], max: [0]), in: r).isEmpty)
        #expect(WaveformGeometry.bars(rows(1, [0], [0]), in: CGRect(x: 0, y: 0, width: 0, height: 10)).isEmpty)
        let nan = WaveformGeometry.bars(rows(1, [.nan, -.infinity], [.infinity, .nan]), in: CGRect(x: 0, y: 0, width: 20, height: 10))
        #expect(nan.allSatisfy { $0.minY.isFinite && $0.height.isFinite && $0.height >= 0.5 })
        let many = WaveformGeometry.bars(
            rows(1, Array(repeating: 0, count: 8192), Array(repeating: 0.1, count: 8192)), in: r)
        #expect(many.count == 8192)
    }

    @Test func regionLabelsSayWhereWhatAndHowLoud() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var r = region(fadeIn: 10, gain: 3)
        #expect(
            RegionA11y.label(r, name: "kick", laneName: "Audio 3", meter: meter, offline: false)
                == "Region kick on Audio 3, bar 1, gain +3.0 dB, fades")
        r.duration = 2 * 61_440
        r.fadeIn = FadeRow(length: 0, shape: .linear)
        r.mute = true
        #expect(
            RegionA11y.label(r, name: "kick", laneName: "Audio 3", meter: meter, offline: true)
                == "Region kick on Audio 3, bars 1 to 2, gain +3.0 dB, offline, muted")
    }

    /// Plan 6a Task 10: a region's own name, else its file's.
    @Test func aRegionsNameComesBeforeItsFilesName() {
        var r = region(fadeIn: 0, gain: 0)
        #expect(RegionA11y.name(r, mediaName: "kick.wav") == "kick.wav")
        #expect(RegionA11y.name(r, mediaName: nil) == "audio")
        r.regionName = "Take 3"
        #expect(RegionA11y.name(r, mediaName: "kick.wav") == "Take 3")
        r.regionName = ""
        #expect(RegionA11y.name(r, mediaName: "kick.wav") == "kick.wav")
    }
}

/// Final review V1, V2, V4: where the gain line and the offline labels go.
@MainActor
@Suite struct RegionDrawingLayoutTests {
    @Test func theGainLineSitsBelowTheSelectionOutline() {
        for rect in [box, CGRect(x: 30, y: 2, width: 120, height: 18)] {
            let g = RegionDrawing.gainLineRect(in: rect)
            // The selected outline is stroked on the path inset by 0.5.
            let outlineBottom = rect.minY + 0.5 + RegionDrawing.outlineWidth(selected: true) / 2
            #expect(g.minY >= outlineBottom, "\(g) under the outline")
            #expect(g.maxY <= rect.minY + RegionHandles.band, "\(g) inside the gain band")
            #expect(g.height >= 1.5 && g.minX > rect.minX && g.maxX < rect.maxX)
        }
    }

    @Test func theOfflinePillClearsTheGainBandAndTheFadeIn() {
        let size = CGSize(width: 36, height: 12)
        let plain = RegionDrawing.offlineLabelRect(region(), in: box, size: size)
        #expect(plain?.minX == box.minX + 4)
        #expect((plain?.minY ?? 0) >= box.minY + RegionHandles.band)
        // After the fade-in handle, so the fade-in line never runs under it.
        let r = region(fadeIn: 12_000)
        let faded = RegionDrawing.offlineLabelRect(r, in: box, size: size)
        #expect((faded?.minX ?? 0) > RegionHandles.fadeInX(r, in: box) + RegionHandles.size / 2)
        // An 18-point strip region has no room: the strip labels it itself.
        #expect(RegionDrawing.offlineLabelRect(region(), in: CGRect(x: 0, y: 2, width: 160, height: 18), size: size) == nil)
        #expect(RegionDrawing.offlineLabelRect(region(), in: CGRect(x: 0, y: 0, width: 30, height: 40), size: size) == nil)
    }

    @Test func theStripLabelSaysOfflineBelowTheGainLine() {
        #expect(RegionDrawing.stripLabel("Audio 3", offline: true) == "Audio 3 — Offline")
        #expect(RegionDrawing.stripLabel("Audio 3", offline: false) == "Audio 3")
        #expect(RegionDrawing.stripLabel("", offline: true) == "Offline")
        let strip = CGRect(x: 70, y: 24, width: 120, height: 18)
        let at = RegionDrawing.stripLabelOrigin(in: strip)
        #expect(at.y >= RegionDrawing.gainLineRect(in: strip).maxY)
        let line = NSFont.systemFont(ofSize: 10).boundingRectForFont.height
        #expect(at.y + line <= strip.maxY + 1, "the label fits the 18-point row")
    }
}

/// A crossfaded region (plan 6c Task 6): `region` with its fade in a
/// crossfade over `overlap` frames of the earlier region.
private func crossfaded(fadeIn: Int64 = 12_000, fadeOut: Int64 = 24_000, overlap: UInt64 = 30_000) -> EventRow {
    var r = region(fadeIn: fadeIn, fadeOut: fadeOut)
    r.crossfade = true
    r.overlapWith = 3
    r.overlapFrames = overlap
    return r
}

/// Plan 6c Task 6: the crossfade's X handle, its drag, shapes and label.
@MainActor
@Suite struct CrossfadeHandleTests {
    @Test func crossfadeHandleReplacesTheFadeIn() {
        let r = crossfaded()
        let handles = RegionHandles.rects(r, in: box).map(\.0)
        #expect(handles == [.crossfade, .fadeOut, .gain])
        #expect(RegionHandles.rects(r, in: box).first?.1.midX == RegionHandles.fadeInX(r, in: box))
        #expect(RegionHandles.hit(CGPoint(x: 20, y: 12), row: r, in: box) == .crossfade)
        #expect(RegionHandles.label(.crossfade) == "Crossfade")
        let plain = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.rects(plain, in: box).map(\.0) == [.fadeIn, .fadeOut, .gain], "unflagged: unchanged")
        #expect(RegionHandles.fadeArea(CGPoint(x: 10, y: 30), row: r, in: box) == .crossfade)
        #expect(RegionHandles.fadeArea(CGPoint(x: 140, y: 30), row: r, in: box) == .fadeOut)
    }

    @Test func crossfadeDragClampsToTheRoom() {
        let r = crossfaded()
        #expect(RegionHandles.crossfadeRoom(r) == 30_000)
        #expect(RegionHandles.fades(r, .crossfade, dx: -1_000, in: box) == (1, 24_000))
        #expect(RegionHandles.fades(r, .crossfade, dx: 1_000, in: box) == (30_000, 24_000), "up to the overlap")
        #expect(RegionHandles.fades(r, .crossfade, dx: 10, in: box) == (18_000, 24_000))
        // The fade out leaves less than the overlap.
        let tight = crossfaded(fadeOut: 80_000, overlap: 30_000)
        #expect(RegionHandles.crossfadeRoom(tight) == 16_000)
        #expect(RegionHandles.fades(tight, .crossfade, dx: 1_000, in: box).fadeIn == 16_000)
        #expect(RegionHandles.fadeCommand(r, .crossfade, dx: 0, in: box) == nil)
        #expect(
            RegionHandles.fadeCommand(r, .crossfade, dx: 1_000, in: box)
                == .audio(edit: .crossfade(event: 4, length: .frames(frames: 30_000), shape: .linear)))
        #expect(
            RegionHandles.target(r, .crossfade, rect: box, dx: -1_000, dy: 5, fine: false)
                == .audio(edit: .crossfade(event: 4, length: .frames(frames: 1), shape: .linear)))
        // Hostile rows: an overlap past Int64 does not trap; no room is 1.
        var huge = crossfaded()
        huge.overlapFrames = .max
        huge.frames = .max
        #expect(RegionHandles.crossfadeRoom(huge) == Int64.max - 24_000)
        #expect(RegionHandles.crossfadeRoom(crossfaded(overlap: 0)) == 0)
        #expect(RegionHandles.fades(crossfaded(overlap: 0), .crossfade, dx: 50, in: box).fadeIn == 1)
        #expect(RegionHandles.crossfadeReadout(r, dx: 1_000, in: box, rate: 48_000) == "625 ms")
        #expect(RegionHandles.crossfadeReadout(r, dx: -1_000, in: box, rate: 48_000) == "0 ms", "1 frame")
    }

    @Test func crossfadeShapeKeepsTheLength() {
        let r = crossfaded()
        #expect(
            RegionHandles.shapeCommand(r, .crossfade, .sCurve)
                == .audio(edit: .crossfade(event: 4, length: .frames(frames: 12_000), shape: .sCurve)))
        #expect(RegionHandles.shapeCommand(r, .crossfade, .linear) == nil, "already that shape")
    }

    @Test func crossfadeLinesMakeAnX() {
        var r = crossfaded(fadeIn: 300, fadeOut: 0, overlap: 500)
        r.frames = 1_000
        let rect = CGRect(x: 0, y: 0, width: 100, height: 40)
        let lines = RegionDrawing.crossfadeLines(r, in: rect)
        #expect(lines.count == 2)
        #expect(lines.first?.0 == CGPoint(x: 0, y: 40) && lines.first?.1 == CGPoint(x: 30, y: 0))
        #expect(lines.last?.0 == CGPoint(x: 0, y: 0) && lines.last?.1 == CGPoint(x: 30, y: 40))
        r.crossfade = false
        #expect(RegionDrawing.crossfadeLines(r, in: rect).isEmpty)
    }

    @Test func crossfadeMenuOffersShapesAndRemove() {
        var r = crossfaded()
        r.fadeIn.shape = .equalPower
        var sent: [EditCommand] = []
        let menu = RegionDrawing.fadeShapeMenu(r, .crossfade) { sent.append($0) }
        #expect(
            menu.items.map(\.title) == [
                "Crossfade: Linear", "Crossfade: Equal Power", "Crossfade: Exponential", "Crossfade: S-Curve",
                "Remove Crossfade",
            ])
        #expect(menu.items.map { $0.state == .on } == [false, true, false, false, false])
        let remove = try! #require(menu.items.last)
        _ = remove.target?.perform(remove.action)
        #expect(sent == [.audio(edit: .removeCrossfade(event: 4))])
        let linear = menu.items[0]
        _ = linear.target?.perform(linear.action)
        #expect(sent.last == .audio(edit: .crossfade(event: 4, length: .frames(frames: 12_000), shape: .linear)))
        // A plain fade's menu is unchanged.
        let plain = RegionDrawing.fadeShapeMenu(region(fadeIn: 10), .fadeIn) { _ in }
        #expect(plain.items.map(\.title).allSatisfy { $0.hasPrefix("Fade In: ") } && plain.items.count == 4)
    }

    @Test func a11yLabelSaysCrossfade() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        #expect(
            RegionA11y.label(crossfaded(fadeOut: 0), name: "kick", laneName: "Audio 3", meter: meter, offline: false)
                == "Region kick on Audio 3, bar 1, gain 0.0 dB, crossfade")
        #expect(
            RegionA11y.label(crossfaded(), name: "kick", laneName: "Audio 3", meter: meter, offline: false)
                == "Region kick on Audio 3, bar 1, gain 0.0 dB, crossfade, fades")
    }
}
