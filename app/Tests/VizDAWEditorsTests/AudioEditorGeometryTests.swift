import AppKit
import CoreGraphics
import Foundation
import Testing
import VizDAWAppModel
import VizDAWCore

@testable import VizDAWEditors

/// A region at media offset 24 000, 48 000 frames long, in a 96 000-frame file.
private func region(fadeIn: Int64 = 0, fadeOut: Int64 = 0, gain: Float = 0, offset: UInt64 = 24_000, frames: UInt64 = 48_000)
    -> EventRow
{
    var r = Pencil.note(tick: 0, pitch: 0, length: 61_440)
    r.id = 4
    r.kind = .audioRegion
    r.media = 7
    r.sourceOffset = offset
    r.frames = frames
    r.gainDb = gain
    r.fadeIn = FadeRow(length: fadeIn, shape: .linear)
    r.fadeOut = FadeRow(length: fadeOut, shape: .equalPower)
    return r
}

private let minus = "\u{2212}"
private let media: UInt64 = 96_000

@Suite struct AudioEditorGeometryTests {
    @Test func trimDragsClampToTheFileAndTheRegion() {
        let r = region()
        func trim(_ h: AudioEditorHandle, _ dx: CGFloat, media m: UInt64 = media, room: UInt64 = 100_000) -> EditCommand? {
            AudioEditorGeometry.trimCommand(r, h, dx: dx, framesPerPoint: 100, mediaFrames: m, startRoom: room)
        }
        func cmd(_ front: Int64, _ back: Int64) -> EditCommand { .audio(edit: .trim(event: 4, frontFrames: front, backFrames: back)) }
        #expect(trim(.start, -1000) == cmd(-24_000, 0))
        #expect(trim(.start, 1000) == cmd(47_999, 0))
        #expect(trim(.start, -1000, room: 10_000) == cmd(-10_000, 0))
        #expect(trim(.start, -1000, room: 0) == nil)
        #expect(trim(.start, 10, room: 0) == cmd(1000, 0))
        #expect(trim(.end, 1000) == cmd(0, -24_000))
        #expect(trim(.end, -1000) == cmd(0, 47_999))
        #expect(trim(.start, 0.004) == nil)
        #expect(trim(.start, .nan) == nil)
        #expect(trim(.start, .infinity) == nil)
        #expect(trim(.end, 10, media: 50_000) == nil)
        #expect(trim(.end, -10, media: 50_000) == cmd(0, 1000))
        #expect(trim(.gain, 10) == nil)
        #expect(AudioEditorGeometry.label(.start) == "Trim Start")
        #expect(AudioEditorGeometry.label(.end) == "Trim End")
    }

    /// 480 points wide at 100 frames per point.
    private let wave = CGRect(x: 44, y: 18, width: 600, height: 100)
    private var axis: FrameAxis { FrameAxis(framesPerPoint: 100, originFrame: 0) }

    private func target(_ r: EventRow, _ h: AudioEditorHandle, dx: CGFloat = 0, dy: CGFloat = 0, fine: Bool = false) -> EditCommand? {
        AudioEditorGeometry.target(
            r, h, dx: dx, dy: dy, fine: fine, axis: axis, mediaFrames: media, startRoom: 100_000, in: wave)
    }

    @Test func fadeDragsStopAtTheOtherFade() {
        let r = region(fadeIn: 12_000, fadeOut: 12_000)
        func fades(_ i: Int64, _ o: Int64) -> EditCommand {
            .audio(
                edit: .setFades(
                    event: 4, fadeIn: FadeRow(length: i, shape: .linear), fadeOut: FadeRow(length: o, shape: .equalPower)))
        }
        #expect(target(r, .fadeIn, dx: 500) == fades(36_000, 12_000))
        #expect(target(r, .fadeOut, dx: -500) == fades(12_000, 36_000))
        #expect(target(r, .fadeIn, dx: -1000) == fades(0, 12_000))
        #expect(target(r, .fadeIn, dx: .nan) == nil)
        #expect(target(r, .fadeIn, dx: 1e300) == fades(36_000, 12_000))
        // M4: the arrangement's own helper no longer traps either.
        let box = CGRect(x: 0, y: 0, width: 480, height: 40)
        #expect(RegionHandles.fades(r, .fadeIn, dx: 1e300, in: box).fadeIn == 36_000)
        #expect(RegionHandles.fades(r, .fadeOut, dx: -.infinity, in: box).fadeOut == 36_000)
        #expect(RegionHandles.fades(r, .fadeIn, dx: .nan, in: box).fadeIn == 12_000)
    }

    @Test func gainDragFollowsTheArrangementsRate() {
        let r = region()
        func gain(_ g: Float) -> EditCommand { .audio(edit: .setGain(event: 4, gainDb: g)) }
        #expect(target(r, .gain, dy: -20) == gain(5.0))
        #expect(target(r, .gain, dy: -20, fine: true) == gain(1.0))
        #expect(target(r, .gain, dy: 10_000) == gain(-144))
        #expect(target(r, .gain, dx: 50) == nil)
        #expect(target(r, .gain, dy: .nan) == nil)
    }

    @Test func handlesOfATinyRegionStayReachable() {
        // 6 points wide: 600 frames at 100 per point.
        let tiny = region(offset: 24_000, frames: 600)
        let w = CGRect(x: 0, y: 0, width: 800, height: 100)
        let ax = FrameAxis(framesPerPoint: 100, originFrame: 0)
        func hit(_ r: EventRow, _ x: CGFloat, _ y: CGFloat) -> AudioEditorHandle? {
            AudioEditorGeometry.hit(CGPoint(x: x, y: y), row: r, axis: ax, in: w)
        }
        let left = ax.x(24_000)
        #expect(hit(tiny, left + 2, 50) == .start)
        #expect(hit(tiny, left + 4, 50) == .end)
        #expect(hit(tiny, left - 3, 50) == .start)
        #expect(!AudioEditorGeometry.handles(tiny, axis: ax, in: w).contains { $0.0 == .fadeIn || $0.0 == .fadeOut })

        // 400 points wide.
        let wide = region(offset: 24_000, frames: 40_000)
        let l = ax.x(24_000)
        #expect(hit(wide, l + 1, 2) == .fadeIn, "the corner, not the gain band")
        #expect(hit(wide, l + 200, 2) == .gain)
        #expect(hit(wide, l, 50) == .start)
        #expect(hit(wide, l + 400, 50) == .end)
        #expect(hit(wide, l + 200, 70) == nil)
        // The gain band: 6 drawn plus 4 slack.
        #expect(hit(wide, l + 200, 9.5) == .gain && hit(wide, l + 200, 10.5) == nil)
        // The right side: the fade-out corner and the end edge's slack.
        #expect(hit(wide, l + 399, 2) == .fadeOut)
        #expect(hit(wide, l + 403, 50) == .end && hit(wide, l + 405, 50) == nil)
        // 16 < width < 24: no fades, and the gain band keeps its width.
        let mid = region(offset: 24_000, frames: 2000)
        #expect(hit(mid, l + 10, 2) == .gain)
        #expect(!AudioEditorGeometry.handles(mid, axis: ax, in: w).contains { $0.0 == .fadeIn })
        // Fades that meet: both corners stay reachable.
        let met = region(fadeIn: 20_000, fadeOut: 20_000, frames: 40_000)
        #expect(hit(met, l + 199, 2) == .fadeIn && hit(met, l + 201, 2) == .fadeOut)
        // Each drawn rect lies inside its hit area.
        for r in [tiny, wide, mid, met] {
            let hits = AudioEditorGeometry.handles(r, axis: ax, in: w)
            for (h, d) in AudioEditorGeometry.drawn(r, axis: ax, in: w) {
                guard let area = hits.first(where: { $0.0 == h })?.1 else { continue }
                #expect(area.contains(CGPoint(x: d.midX, y: d.midY)) && area.intersection(d) == d.intersection(w))
            }
        }
        // I2: nothing reaches into the gutter or the ruler.
        let shifted = FrameAxis(framesPerPoint: 100, originFrame: 30_000)
        #expect(AudioEditorGeometry.hit(CGPoint(x: 30, y: 20), row: region(), axis: shifted, in: self.wave) == nil)
        #expect(AudioEditorGeometry.hit(CGPoint(x: 60, y: 20), row: region(), axis: shifted, in: self.wave) == .gain)
        #expect(AudioEditorGeometry.hit(CGPoint(x: 284, y: 16), row: region(), axis: axis, in: self.wave) == nil)
    }

    @Test func scaleMarksNeverOverlap() {
        func labels(_ h: CGFloat, amp: Int = 1, ch: Int = 1) -> [String] {
            AudioEditorGeometry.scaleMarks(amplitude: amp, channels: ch, in: CGRect(x: 0, y: 0, width: 500, height: h))
                .map(\.label)
        }
        #expect(labels(40) == ["0", "\(minus)\u{221E}"])
        #expect(
            labels(600) == ["0", "\(minus)3", "\(minus)6", "\(minus)9", "\(minus)12", "\(minus)18", "\(minus)24", "\(minus)\u{221E}"])
        #expect(!labels(600, amp: 2).contains("0"))
        for amp in AudioEditorGeometry.amplitudes {
            for h in [CGFloat(40), 120, 600] {
                for ch in [1, 2] {
                    let wave = CGRect(x: 0, y: 5, width: 500, height: h)
                    let marks = AudioEditorGeometry.scaleMarks(amplitude: amp, channels: ch, in: wave)
                    let band = h / CGFloat(ch)
                    for k in 0..<ch {
                        let lo = wave.minY + band * CGFloat(k)
                        let inBand = marks.filter { $0.y >= lo - 1e-9 && $0.y < lo + band - 1e-9 }
                        #expect(!inBand.isEmpty)
                        for m in inBand { #expect(m.y <= lo + band / 2 + 1e-9) }
                        for (a, b) in zip(inBand, inBand.dropFirst()) {
                            #expect(b.y - a.y >= AudioEditorGeometry.minMarkGap - 1e-9, "amp \(amp) h \(h) ch \(ch)")
                        }
                    }
                }
            }
        }
    }

    @Test func rulerMarksAreEvenAndLabelled() {
        let ax = FrameAxis(framesPerPoint: 100, originFrame: 0)
        let marks = AudioEditorGeometry.rulerMarks(axis: ax, width: 600, sampleRate: 48_000)
        #expect(marks.count == 7)
        for (i, m) in marks.enumerated() { #expect(abs(m.x - CGFloat(i) * 96) < 1e-6) }
        #expect(marks.first?.label == "0:00.000")
        #expect(marks[1].label == "0:00.200")
        #expect(marks.last?.label == "0:01.200")
        #expect(AudioEditorGeometry.rulerMarks(axis: ax, width: 600, sampleRate: 0).isEmpty)
        // Before the file's start there are no marks.
        let early = AudioEditorGeometry.rulerMarks(
            axis: FrameAxis(framesPerPoint: 100, originFrame: -9600), width: 600, sampleRate: 48_000)
        #expect(early.first?.label == "0:00.000" && abs((early.first?.x ?? 0) - 96) < 1e-6)
        // Far zoom-out stays bounded.
        let far = AudioEditorGeometry.rulerMarks(axis: FrameAxis(framesPerPoint: 1e12), width: 600, sampleRate: 48_000)
        #expect(far.count <= 1000)
    }

    @Test func waveformScalesByEnvelopeAndAmplitude() {
        let rows = OverviewRows(
            state: .ready, channels: 1, columns: 4, min: [-0.5, -0.25, 0, -1], max: [0.5, 0.25, 0, 1])
        let env: [Float] = [0, 0.5, 1, 2]
        let s = AudioEditorGeometry.shaped(rows, envelope: env, amplitude: 2)
        #expect(s.max == [0, 0.25, 0, 4])
        #expect(s.min == [0, -0.25, 0, -4])
        #expect(AudioEditorGeometry.clipColumns(rows, envelope: env) == [3])
        let bad = AudioEditorGeometry.shaped(rows, envelope: [1, 1, 1], amplitude: 2)
        #expect(bad.columns == 4 && bad.max == [0, 0, 0, 0] && bad.min == [0, 0, 0, 0])
        let plain = AudioEditorGeometry.shaped(rows, envelope: nil, amplitude: 4)
        #expect(plain.max == [2, 1, 0, 4])
        let stereo = OverviewRows(
            state: .ready, channels: 2, columns: 2, min: [-1, -1, -0.5, -0.5], max: [1, 1, 0.5, 0.5])
        let st = AudioEditorGeometry.shaped(stereo, envelope: [2, 0.5], amplitude: 1)
        #expect(st.max == [2, 0.5, 1, 0.25] && st.min == [-2, -0.5, -1, -0.25])
        #expect(AudioEditorGeometry.clipColumns(stereo, envelope: [2, 0.5]) == [0])
        // Per band (Task 6 review M3): only the channel that clips.
        #expect(AudioEditorGeometry.clipColumns(stereo, envelope: [2, 0.5], channel: 0) == [0])
        #expect(AudioEditorGeometry.clipColumns(stereo, envelope: [2, 0.5], channel: 1) == [])
        let right = OverviewRows(
            state: .ready, channels: 2, columns: 2, min: [-0.5, -0.5, -1, -1], max: [0.5, 0.5, 1, 1])
        #expect(AudioEditorGeometry.clipColumns(right, envelope: [2, 0.5], channel: 0) == [])
        #expect(AudioEditorGeometry.clipColumns(right, envelope: [2, 0.5], channel: 1) == [0])
        #expect(AudioEditorGeometry.clipColumns(right, envelope: [2, 0.5], channel: 2) == [])
        let line = AudioEditorGeometry.envelopeLine(
            [0, 1], amplitude: 1, channel: 0, channels: 1, in: CGRect(x: 0, y: 0, width: 10, height: 100))
        #expect(line == [CGPoint(x: 2.5, y: 50), CGPoint(x: 7.5, y: 0)])
        let second = AudioEditorGeometry.envelopeLine(
            [0.5, .nan, 4], amplitude: 1, channel: 1, channels: 2, in: CGRect(x: 0, y: 0, width: 9, height: 100))
        #expect(second == [CGPoint(x: 1.5, y: 62.5), CGPoint(x: 4.5, y: 75), CGPoint(x: 7.5, y: 50)])
        #expect(AudioEditorGeometry.envelopeLine([1], amplitude: 1, channel: 2, channels: 2, in: CGRect(x: 0, y: 0, width: 9, height: 9)).isEmpty)
    }

    @Test func columnsNeverExceedTheCoreLimit() {
        #expect(AudioEditorGeometry.columns(width: 0) == 1)
        #expect(AudioEditorGeometry.columns(width: -5) == 1)
        #expect(AudioEditorGeometry.columns(width: .nan) == 1)
        #expect(AudioEditorGeometry.columns(width: 600.2) == 601)
        #expect(AudioEditorGeometry.columns(width: 1e9) == 8192)
    }

    @Test func spanStaysInsideTheFile() {
        func span(_ origin: Double, fpp: Double = 88, media m: UInt64 = 96_000) -> AudioEditorGeometry.FrameSpan? {
            AudioEditorGeometry.span(axis: FrameAxis(framesPerPoint: fpp, originFrame: origin), width: 600, mediaFrames: m)
        }
        let a = span(-26_400)
        #expect(a?.from == 0 && a?.to == 26_400 && a?.x == 300 && a?.width == 300)
        let b = span(90_000)
        #expect(b?.from == 90_000 && b?.to == 96_000 && b?.x == 0)
        #expect(abs((b?.width ?? 0) - 68.18) < 0.01)
        #expect(span(100_000) == nil)
        #expect(span(0, media: 0) == nil)
        #expect(span(.nan) == nil)
        #expect(span(0, fpp: .nan) == nil)
        #expect(span(-1e300) == nil)
        #expect(span(1e300) == nil)
        // M3: scrolling or zooming a NaN-origin axis leaves a finite one (it no longer stays NaN).
        let nan = FrameAxis(framesPerPoint: 88, originFrame: .nan)
        for repaired in [
            nan.scrolled(byPoints: .nan, width: 600, mediaFrames: 96_000),
            nan.zoomed(by: 2, aroundX: .nan, width: 600, mediaFrames: 96_000),
        ] {
            #expect(repaired.originFrame.isFinite && repaired.framesPerPoint.isFinite)
            #expect(AudioEditorGeometry.span(axis: repaired, width: 600, mediaFrames: 96_000) != nil)
        }
        #expect(AudioEditorGeometry.span(axis: FrameAxis(), width: .infinity, mediaFrames: 96_000) == nil)
    }
}
