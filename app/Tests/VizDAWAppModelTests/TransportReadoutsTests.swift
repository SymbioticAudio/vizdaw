import AppKit
import Foundation
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The transport bar's LCD readouts (plan 7d Task B4).
@MainActor
@Suite struct TransportReadoutsTests {
    private func state(
        bbu: String = "12.3.480", seconds: Double = 22.875, tps: Double = 30_720, tick: Int64 = 0
    ) -> TransportState {
        TransportState(
            playing: false, seq: nil, tick: tick, positionBbu: bbu, seconds: seconds, sampleRate: 0,
            device: "", faults: [], report: [], hostTimeNs: 0, ticksPerSecond: tps)
    }

    private func meter(_ entries: (Int64, UInt8, UInt8)...) -> [MeterEntryRow] {
        entries.map { MeterEntryRow(tick: $0.0, num: $0.1, den: $0.2, sharps: 0, minor: false) }
    }

    private func hop() async {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            DispatchQueue.main.async { c.resume() }
        }
    }

    @Test func theReadoutsReadThePositionTimeTempoAndMeter() {
        let m = meter((0, 4, 4))
        #expect(
            TransportReadouts.texts(state(), meter: m)
                == ReadoutTexts(bar: "12.3.480", time: "0:22.875", tempo: "120.00", signature: "4/4"))
        #expect(TransportReadouts.texts(state(tps: 23_040), meter: m).tempo == "90.00")
        #expect(TransportReadouts.Box.allCases.map(\.title) == ["Bar", "Time", "Tempo", "Sig"])
        #expect(
            TransportReadouts.Box.allCases.map(\.accessibilityLabel)
                == ["Bar Readout", "Time Readout", "Tempo Readout", "Signature Readout"])
    }

    @Test func theSignatureFollowsTheMeterMap() {
        let m = meter((0, 4, 4), (122_880, 7, 8))
        func sig(_ tick: Int64, _ meter: [MeterEntryRow]?) -> String {
            TransportReadouts.texts(state(tick: tick), meter: meter).signature
        }
        #expect(sig(122_879, m) == "4/4")
        #expect(sig(122_880, m) == "7/8")
        #expect(sig(-1, m) == "4/4", "the pickup")
        #expect(sig(0, nil) == "–")
    }

    @Test func hostileTransportValuesReadAsDashes() async throws {
        func tempo(_ tps: Double) -> String { TransportReadouts.tempoText(ticksPerSecond: tps) }
        for bad in [0, -5, Double.nan, .infinity, -.infinity] { #expect(tempo(bad) == "–", "\(bad)") }
        #expect(tempo(256_000) == "–", "1000 BPM")
        #expect(tempo(1e12) == "–")
        #expect(tempo(999.99 * 256) == "999.99")
        #expect(tempo(999.996 * 256) == "–", "rounds to 1000.00")
        #expect(TransportReadouts.texts(state(seconds: .nan), meter: nil).time == "--:--.---")
        #expect(TransportReadouts.texts(state(bbu: ""), meter: nil).bar == "-.-.-")
        let four = meter((0, 4, 4))
        for tick in [Int64.min, Int64.max] {
            #expect(TransportReadouts.texts(state(tick: tick), meter: four).signature == "4/4")
        }
        #expect(TransportReadouts.tickLimit == TimeAxis.limit, "the same clamp as the time axis")
        // A Bar text past the widest is cut in a box of the fixed width.
        let f = demoFake()
        let doc = try DocumentModel(session: f)
        let v = TransportReadoutsView(document: doc)
        v.frame = NSRect(origin: .zero, size: v.intrinsicContentSize)
        v.layoutSubtreeIfNeeded()
        let bar = try #require(v.valueField(.bar))
        let before = bar.frame.width
        f.state.positionBbu = "123456.4.960"
        doc.poll()
        await hop()
        v.layoutSubtreeIfNeeded()
        #expect(bar.stringValue == "123456.4.960")
        #expect(bar.lineBreakMode == .byTruncatingTail)
        #expect(bar.frame.width == v.valueWidth(.bar) && bar.frame.width == before)
        #expect(v.frame.width == v.intrinsicContentSize.width, "the group keeps its width")
        #expect(v.valueWidth(.bar) > 0)
    }

    /// Final wave (B4 carry): the widest text of each box fits its value
    /// field in every theme, so nothing is cut at the tail; the signature
    /// takes the widest meter a file can hold, 255/128.
    @Test func theWidestTextFitsEachValueFieldInEveryTheme() throws {
        let doc = try DocumentModel(session: demoFake())
        let v = TransportReadoutsView(document: doc)
        #expect(TransportReadouts.widest.signature == "255/128")
        for t in Theme.all {
            v.theme = t
            v.frame = NSRect(origin: .zero, size: v.intrinsicContentSize)
            v.layoutSubtreeIfNeeded()
            for b in TransportReadouts.Box.allCases {
                let field = try #require(v.valueField(b))
                let saved = field.stringValue
                for text in [TransportReadouts.widest.text(b), b == .signature ? "255/128" : saved] {
                    field.stringValue = text
                    let need = ceil(field.intrinsicContentSize.width)
                    #expect(field.frame.width >= need, "\(t.id) \(b) \(text): \(field.frame.width) < \(need)")
                }
                field.stringValue = saved
            }
        }
    }

    /// Final wave (B4 carry): a theme set again, unchanged, measures and
    /// lays out nothing.
    @Test func aThemeSetAgainUnchangedIsNotAppliedAgain() throws {
        let v = TransportReadoutsView(document: try DocumentModel(session: demoFake()))
        v.theme = .classicLight
        let n = v.themeApplications
        v.theme = .classicLight
        #expect(v.themeApplications == n)
        v.theme = .flat
        #expect(v.themeApplications == n + 1)
    }

    @Test func theReadoutLookFollowsTheTheme() {
        for t in Theme.all {
            let look = TransportReadouts.look(t)
            if t.id == Theme.flat.id {
                #expect(look.fill == t.accent.accent && look.value == t.accent.onAccent)
                #expect(look.edge == nil && look.gap == t.spacing.xs)
            } else {
                #expect(look.fill == t.readout.background && look.value == t.readout.text, "\(t.id)")
                #expect(look.edge == t.surface.separator && look.gap == 1, "\(t.id)")
            }
            #expect(look.radius == t.radius.m && look.padding == t.spacing.m)
            let fill = look.fill.nsColor
            let value = Theme.contrast(look.value.nsColor, fill)
            let label = Theme.contrast(look.label.nsColor, fill)
            #expect(value >= 4.5, "\(t.id) value \(value)")
            if t.id == Theme.eg13.id {
                #expect(label >= 4.3 && label < 4.5, "\(t.id) label is the listed exception: \(label)")
            } else {
                #expect(label >= 4.5, "\(t.id) label \(label)")
            }
        }
        #expect(TransportReadouts.label("Bar", .flat) == "bar")
        #expect(TransportReadouts.label("Bar", .classicLight) == "BAR")
    }

    @Test func theFlowWrapsReadoutsAboveTheFieldsWhenNarrow() {
        func lines(_ w: [CGFloat], _ width: CGFloat) -> [[Int]] {
            TransportFlow.lines(widths: w, lastMinimum: 660, width: width, spacing: 12)
        }
        let all: [CGFloat] = [80, 330, 700]
        #expect(lines(all, 1200) == [[0, 1, 2]])
        #expect(lines(all, 900) == [[0, 1], [2]])
        #expect(lines(all, 400) == [[0], [1], [2]])
        #expect(lines(all, 0) == [[0], [1], [2]])
        #expect(lines(all, .nan) == [[0], [1], [2]])
        #expect(lines([330, 700], 1100) == [[0, 1]])
        #expect(lines([330, 700], 700) == [[0], [1]])
    }

    @Test func theReadoutsViewNamesItsBoxes() async throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        let v = TransportReadoutsView(document: m)
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 80), styleMask: [.titled], backing: .buffered,
            defer: true)
        w.isReleasedWhenClosed = false
        defer { w.close() }
        w.contentView = v
        func children() throws -> [NSAccessibilityElementProtocol & NSObjectProtocol] {
            try #require(v.accessibilityChildren() as? [NSAccessibilityElementProtocol & NSObjectProtocol])
        }
        func values() throws -> [String] {
            try children().map { (($0 as? NSView)?.accessibilityValue() as? String) ?? "" }
        }
        let kids = try children()
        #expect(kids.count == 4)
        #expect(
            kids.map { ($0 as? NSView)?.accessibilityLabel() } == TransportReadouts.Box.allCases.map(\.accessibilityLabel))
        let texts = m.readoutTexts
        #expect(try values() == [texts.bar, texts.time, texts.tempo, texts.signature])
        let before = try values()[0]
        f.state.tick = 61_440
        f.state.positionBbu = "2.1.0"
        m.poll()
        await hop()
        let after = try values()[0]
        #expect(after != before && after == m.readoutTexts.bar)
        v.theme = .flat
        let look = TransportReadouts.look(.flat)
        for b in TransportReadouts.Box.allCases {
            let value = try #require(v.valueField(b))
            #expect(value.textColor == look.value.nsColor, "\(b)")
        }
        #expect(v.boxes[.bar]?.title.stringValue == "bar")
    }
}
