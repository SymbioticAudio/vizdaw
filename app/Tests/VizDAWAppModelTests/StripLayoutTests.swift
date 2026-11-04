import CoreGraphics
import Testing
@testable import VizDAWAppModel

@Suite struct StripLayoutTests {
    @Test func theFaderGetsALongThrowWhenThereIsRoom() {
        let s = StripLayout.split(height: 900, upper: 300)
        #expect(s.throwLength == min(480, 900 - 300 - StripLayout.lowerFixed))
        #expect(!s.scrolls)
    }

    @Test func aShortPaneScrollsTheStripAndKeepsTheMinimumThrow() {
        #expect(StripLayout.split(height: 260, upper: 300) == StripSplit(throwLength: 160, scrolls: true))
        #expect(StripLayout.split(height: 120, upper: 0) == StripSplit(throwLength: 160, scrolls: true))
        let edge = StripLayout.split(height: 300 + StripLayout.lowerFixed + 160, upper: 300)
        #expect(edge == StripSplit(throwLength: 160, scrolls: false))
    }

    @Test func theDbScaleMarksSitWhereTheFaderPutsThem() {
        let m = StripLayout.faderMarks
        #expect(m.map(\.label) == ["+12", "+6", "0", "\u{2212}6", "\u{2212}12", "\u{2212}24", "\u{2212}36", "\u{2212}48", "\u{2212}\u{221E}"])
        for (a, b) in zip(m, m.dropFirst()) { #expect(a.position > b.position) }
        #expect(m[2].position == 60.0 / 72)
        #expect(m.last?.position == 0)
        #expect(m.filter { $0.position == 0 }.count == 1)
        #expect(StripLayout.width == 120 && StripLayout.minimumThrow == 160 && StripLayout.maximumThrow == 480)
    }
}

/// B6 review carries (plan 7b Task B7): the fader's cap and the level box's
/// 24-point hit frame are in the strip's fixed height, so a short pane
/// scrolls rather than overflows.
@Suite struct StripHeightTests {
    @Test func theFixedLowerPartHoldsTheFaderCapAndTheLevelBoxHitFrame() {
        #expect(StripLayout.faderCap == 52)
        #expect(StripLayout.levelBoxHeight == 24)
        #expect(StripLayout.toggleRowHeight == 24)
        #expect(StripLayout.modeMenuHeight == 24, "the 24-pt hit minimum (review M2)")
        let parts = StripLayout.faderCap + StripLayout.levelBoxHeight + StripLayout.toggleRowHeight
            + StripLayout.modeMenuHeight + 4 * StripLayout.gap + 2 * StripLayout.padding
        #expect(StripLayout.lowerFixed == parts)
        #expect(StripLayout.contentHeight(upper: 300, throwLength: 160) == 300 + 160 + parts)
    }

    @Test func atEveryPaneHeightTheStripFitsOrScrolls() {
        for upper in stride(from: CGFloat(0), through: 640, by: 37) {
            for height in stride(from: CGFloat(0), through: 1400, by: 7) {
                let s = StripLayout.split(height: height, upper: upper)
                let content = StripLayout.contentHeight(upper: upper, throwLength: s.throwLength)
                #expect(s.scrolls || content <= height, "height \(height), upper \(upper)")
            }
        }
        // At the shortest pane that does not scroll, the content fills it exactly.
        let upper: CGFloat = 300
        let minimum = upper + StripLayout.lowerFixed + StripLayout.minimumThrow
        let s = StripLayout.split(height: minimum, upper: upper)
        #expect(!s.scrolls)
        #expect(StripLayout.contentHeight(upper: upper, throwLength: s.throwLength) == minimum)
        #expect(StripLayout.split(height: minimum - 1, upper: upper).scrolls)
    }
}

/// Fix round 1 (controller): every strip of one mixer shares its rows, so
/// faders, level boxes and buttons line up whatever each strip holds.
@Suite struct StripGridTests {
    @Test func stripsWithAndWithoutInputAndSendsShareTheirRows() {
        // An instrument strip with sends, an audio path with an input and
        // two inserts, a bus with neither, and master with no sends or pan.
        let synth: [StripRowKind: CGFloat] = [.header: 40, .eq: 20, .inserts: 36, .sends: 80, .output: 20, .pan: 38]
        let audio: [StripRowKind: CGFloat] = [.header: 40, .input: 22, .eq: 20, .inserts: 58, .sends: 20, .output: 20, .pan: 38]
        let bus: [StripRowKind: CGFloat] = [.header: 40, .eq: 20, .inserts: 36, .output: 20, .pan: 38]
        let master: [StripRowKind: CGFloat] = [.header: 44, .eq: 20, .inserts: 36]
        let rows = StripGrid.rowHeights([synth, audio, bus, master])
        #expect(rows[.header] == 44 && rows[.input] == 22 && rows[.inserts] == 58 && rows[.sends] == 80)
        #expect(rows[.instrument] == nil, "no strip has the row: it takes no space")
        let tops = StripGrid.tops(rows)
        // The same table for every strip: each row starts at one y.
        #expect(tops[.header] == 0)
        #expect(tops[.input] == 44 + StripGrid.rowGap)
        #expect(tops[.eq] == 44 + 22 + 2 * StripGrid.rowGap)
        let upper = StripGrid.upper(rows)
        #expect(upper == 44 + 22 + 20 + 58 + 80 + 20 + 38 + 6 * StripGrid.rowGap)
        let faderTop = StripGrid.faderTop(rows)
        #expect(faderTop == StripLayout.padding + upper + StripLayout.gap)
        // A strip's own rows alone would put its fader elsewhere; the grid does not.
        #expect(StripGrid.faderTop(StripGrid.rowHeights([bus])) < faderTop)
        for one in [synth, audio, bus, master] {
            #expect(StripGrid.faderTop(StripGrid.rowHeights([one])) <= faderTop)
        }
        #expect(StripGrid.upper([:]) == 0)
    }
}

/// Review M1: the grid's own placement (what the mixer view uses) puts
/// every strip's fader, level box and buttons at one y.
@Suite struct StripGridPlacementTests {
    func cells() -> [StripGrid.Cell] {
        // Column 0: synth with sends; 1: audio with an input; 2: bus with
        // neither; 3: master (no sends, output or pan), after a divider.
        var c: [StripGrid.Cell] = []
        let rows: [[(StripRowKind, CGFloat)]] = [
            [(.header, 40), (.eq, 20), (.inserts, 36), (.sends, 80), (.output, 24), (.pan, 38)],
            [(.header, 40), (.input, 22), (.eq, 20), (.inserts, 58), (.output, 24), (.pan, 38)],
            [(.header, 40), (.eq, 20), (.inserts, 36), (.output, 24), (.pan, 38)],
            [(.header, 44), (.eq, 20), (.inserts, 36)],
        ]
        for (i, r) in rows.enumerated() {
            c.append(StripGrid.Cell(column: i, part: .panel))
            for (k, h) in r { c.append(StripGrid.Cell(column: i, part: .row(k), measured: h)) }
            for p in [StripGrid.Part.fader, .level, .buttons, .mode] { c.append(StripGrid.Cell(column: i, part: p)) }
        }
        c.append(StripGrid.Cell(column: 0, part: .divider))
        return c
    }

    @Test func everyStripsFaderLevelAndButtonsShareOneY() {
        let cells = cells()
        let p = StripGrid.place(cells, height: 700, dividerBefore: 3)
        for part in [StripGrid.Part.fader, .level, .buttons, .mode] {
            let ys = Set(zip(cells, p.frames).filter { $0.0.part == part }.map { $0.1.minY })
            #expect(ys.count == 1, "\(part): \(ys)")
        }
        for k in StripRowKind.allCases {
            let ys = Set(zip(cells, p.frames).filter { $0.0.part == .row(k) }.map { $0.1.minY })
            #expect(ys.count <= 1, "\(k): \(ys)")
        }
        let fader = zip(cells, p.frames).first { $0.0.part == .fader }!.1
        #expect(fader.minY == StripGrid.faderTop(StripGrid.rowHeights([
            [.header: 44, .input: 22, .eq: 20, .inserts: 58, .sends: 80, .output: 24, .pan: 38]
        ])))
        // The divider sits in the middle of the gap before master (review M5).
        let divider = zip(cells, p.frames).first { $0.0.part == .divider }!.1
        let busRight = zip(cells, p.frames).first { $0.0.part == .panel && $0.0.column == 2 }!.1.maxX
        let masterLeft = zip(cells, p.frames).first { $0.0.part == .panel && $0.0.column == 3 }!.1.minX
        #expect(abs(divider.midX - (busRight + masterLeft) / 2) < 0.01)
        #expect(p.size.height == StripLayout.contentHeight(upper: StripGrid.upper(StripGrid.rowHeights([
            [.header: 44, .input: 22, .eq: 20, .inserts: 58, .sends: 80, .output: 24, .pan: 38]
        ])), throwLength: fader.height - StripLayout.faderCap))
    }
}
