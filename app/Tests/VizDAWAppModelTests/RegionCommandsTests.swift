import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Region menu (plan 5b Task 8; spec §10.2).
@MainActor
@Suite struct RegionCommandsTests {
    func model() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        return (try DocumentModel(session: f), f)
    }

    @Test func everyCommandNeedsSelectedEvents() throws {
        let (m, f) = try model()
        for id in RegionCommands.ids.subtracting(RegionCommands.crossfadeIds) {
            #expect(!RegionCommands.enabled(id, m))
            #expect(RegionCommands.run(id, m) == .unavailable("Select audio regions first."), "\(id)")
        }
        #expect(f.edits.isEmpty && f.normalizations.isEmpty)
    }

    @Test func fadesUseTheTransportPosition() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        f.state.tick = 960
        m.poll()
        #expect(RegionCommands.enabled(RegionCommands.fadeIn, m))
        #expect(RegionCommands.run(RegionCommands.fadeIn, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: true)))
        #expect(RegionCommands.run(RegionCommands.fadeOut, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: false)))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .done)
        #expect(f.edits.last == .audio(edit: .removeFades(events: [5])))
        f.editError = .Model(message: "the cursor is not inside a selected audio region")
        #expect(
            RegionCommands.run(RegionCommands.fadeIn, m)
                == .refused("the cursor is not inside a selected audio region"))
    }

    @Test func normalizeSendsThePeakAndReportsBusy() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5, 6]))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .done)
        #expect(f.normalizations.last?.events == [5, 6])
        #expect(f.normalizations.last?.peakDb == -1)
        #expect(RegionCommands.run(RegionCommands.normalize, m) == .done)
        #expect(f.normalizations.last?.peakDb == Float(RegionCommands.defaultPeakDb))
        f.normalizeError = .Busy(message: "still being built")
        #expect(RegionCommands.normalize(peakDb: -1, m) == .busy("still being built"))
        #expect(m.editor.status == "still being built")
        f.normalizeError = nil
        for bad in [0.5, -96.5, Double.nan, Double.infinity] {
            if case .refused = RegionCommands.normalize(peakDb: bad, m) {} else {
                Issue.record("\(bad) was accepted")
            }
        }
        #expect(f.normalizations.count == 2)
    }

    @Test func peakTextParses() {
        #expect(RegionCommands.parsePeak("-0.3") == -0.3)
        #expect(RegionCommands.parsePeak(" −1 ") == -1)
        #expect(RegionCommands.parsePeak("0") == 0)
        for bad in ["", "abc", "nan", "inf", "-inf", "1", "-96.01", "1e999", "0x10", "-0.3dB"] {
            #expect(RegionCommands.parsePeak(bad) == nil, "\(bad)")
        }
    }

    @Test func aGestureBlocksTheCommands() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        #expect(m.editor.beginGesture("Drag", lanes: [], origin: .other))
        #expect(!RegionCommands.enabled(RegionCommands.removeFades, m))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(f.normalizations.isEmpty)
    }

    // MARK: Crossfades (plan 6c Task 5)

    // Track 3/30 holds A (10, from tick 0) and B (11) which starts inside it;
    // 3/31 holds the note (1) and a lone region C (12).
    func crossfadeModel() throws -> (DocumentModel, FakeSession) {
        let (m, f) = try model()
        let a = regionRow(10, tick: 0, duration: 61_440, frames: 96_000)
        var b = regionRow(11, tick: 30_720, duration: 61_440, frames: 96_000)
        b.overlapWith = 10
        b.overlapFrames = 24_000
        b.crossfade = true
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [a, b])
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0), regionRow(12, tick: 0, duration: 100, frames: 10)])
        return (m, f)
    }

    func pick(_ m: DocumentModel, _ events: [UInt64], lanes: [LaneKey] = [LaneKey(3, 30)]) {
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: lanes.map(\.ref), events: events))
    }

    @Test func crossfadeTargetsTheLaterRegion() throws {
        let (m, f) = try crossfadeModel()
        for events: [UInt64] in [[11], [10, 11], [11, 10]] {
            pick(m, events)
            #expect(RegionCommands.crossfadeTarget(m)?.row.id == 11, "\(events)")
            #expect(RegionCommands.crossfadeTarget(m)?.lane == LaneKey(3, 30))
        }
        pick(m, [10])
        #expect(RegionCommands.crossfadeTarget(m) == nil)
        pick(m, [])
        #expect(RegionCommands.crossfadeTarget(m) == nil)
        pick(m, [1], lanes: [LaneKey(3, 31)])
        #expect(RegionCommands.crossfadeTarget(m) == nil, "the note")
        pick(m, [11, 12], lanes: [LaneKey(3, 30), LaneKey(3, 31)])
        #expect(RegionCommands.crossfadeTarget(m) == nil, "two tracks")
        pick(m, [10, 11, 12], lanes: [LaneKey(3, 30), LaneKey(3, 31)])
        #expect(RegionCommands.crossfadeTarget(m) == nil)
        #expect(f.edits.isEmpty)
    }

    @Test func menuCrossfadeSendsTheDefaultUpTo() throws {
        let (m, f) = try crossfadeModel()
        pick(m, [10, 11])
        #expect(RegionCommands.enabled(RegionCommands.crossfade, m))
        #expect(RegionCommands.crossfade(m, defaultMs: 10) == .done)
        #expect(f.edits == [.audio(edit: .crossfade(event: 11, length: .upTo(frames: 480), shape: .equalPower))])
        pick(m, [10])
        #expect(!RegionCommands.enabled(RegionCommands.crossfade, m))
        #expect(RegionCommands.crossfade(m, defaultMs: 10) == .unavailable(RegionCommands.noCrossfadeTarget))
        #expect(f.edits.count == 1)
    }

    @Test func crossfadeChannelBuildsTheEdit() throws {
        let (m, f) = try crossfadeModel()
        let ids: [ControlArg] = [
            ControlArg(name: "seq", value: .int(value: 3)), ControlArg(name: "lane", value: .int(value: 30)),
            ControlArg(name: "event", value: .int(value: 11)),
        ]
        let out = RegionCommands.runChannel(
            RegionCommands.crossfade,
            args: ids + [
                ControlArg(name: "length_ms", value: .text(value: "100")), ControlArg(name: "shape", value: .text(value: "linear")),
            ], doc: UUID(), m, defaultMs: 10)
        guard case .done = out else {
            Issue.record("\(out)")
            return
        }
        #expect(f.edits.last == .audio(edit: .crossfade(event: 11, length: .frames(frames: 4_800), shape: .linear)))
        for (name, shape) in [("equal_power", FadeShapeRow.equalPower), ("exponential", .exponential), ("s_curve", .sCurve)] {
            _ = RegionCommands.runChannel(
                RegionCommands.crossfade, args: ids + [ControlArg(name: "shape", value: .text(value: name))], doc: UUID(), m,
                defaultMs: 10)
            #expect(f.edits.last == .audio(edit: .crossfade(event: 11, length: .upTo(frames: 480), shape: shape)))
        }
        // No arguments: the selection, as the menu.
        pick(m, [10, 11])
        _ = RegionCommands.runChannel(RegionCommands.crossfade, args: [], doc: UUID(), m, defaultMs: 20)
        #expect(f.edits.last == .audio(edit: .crossfade(event: 11, length: .upTo(frames: 960), shape: .equalPower)))
        // Remove, by ids.
        let r = RegionCommands.runChannel(RegionCommands.removeCrossfade, args: ids, doc: UUID(), m, defaultMs: 10)
        guard case .done = r else {
            Issue.record("\(r)")
            return
        }
        #expect(f.edits.last == .audio(edit: .removeCrossfade(event: 11)))
    }

    @Test func crossfadeChannelRefusesHostileArguments() throws {
        let (m, f) = try crossfadeModel()
        func ids(_ seq: Int64, _ lane: Int64, _ event: Int64) -> [ControlArg] {
            [
                ControlArg(name: "seq", value: .int(value: seq)), ControlArg(name: "lane", value: .int(value: lane)),
                ControlArg(name: "event", value: .int(value: event)),
            ]
        }
        func refused(_ id: String, _ args: [ControlArg], _ param: String, _ why: String) {
            let out = RegionCommands.runChannel(id, args: args, doc: UUID(), m, defaultMs: 10)
            guard case .failed(let f) = out, f.code == .invalidParams else {
                Issue.record("\(why): \(out)")
                return
            }
            #expect(f.message.contains(id) && f.message.contains("`\(param)`"), "\(why): \(f.message)")
        }
        for bad in ["0", "-5", "NaN", "inf", "1e400", "", "abc", "3600001", "0x10", " "] {
            refused(
                RegionCommands.crossfade, ids(3, 30, 11) + [ControlArg(name: "length_ms", value: .text(value: bad))],
                "length_ms", bad)
        }
        refused(
            RegionCommands.crossfade, ids(3, 30, 11) + [ControlArg(name: "length_ms", value: .int(value: 5))], "length_ms", "int")
        refused(
            RegionCommands.crossfade, [ControlArg(name: "shape", value: .text(value: "cubic"))], "shape", "cubic")
        refused(RegionCommands.crossfade, [ControlArg(name: "shape", value: .int(value: 1))], "shape", "int shape")
        let all = ids(3, 30, 11)
        for (part, missing) in [(Array(all.prefix(2)), "event"), ([all[2]], "seq"), ([all[0]], "lane"), ([all[1]], "seq")] {
            for id in [RegionCommands.crossfade, RegionCommands.removeCrossfade] {
                refused(id, part, missing, "partial \(part.map(\.name)) \(id)")
                let out = RegionCommands.runChannel(id, args: part, doc: UUID(), m, defaultMs: 10)
                if case .failed(let fail) = out { #expect(fail.message.contains("`\(missing)` is missing"), "\(fail.message)") }
            }
        }
        let twice = [ControlArg(name: "length_ms", value: .text(value: "5")), ControlArg(name: "length_ms", value: .text(value: "6"))]
        refused(RegionCommands.crossfade, all + twice, "length_ms", "duplicated length_ms")
        refused(RegionCommands.crossfade, all + [all[0]], "seq", "duplicated seq")
        refused(RegionCommands.crossfade, ids(3, 30, Int64.max), "event", "event at Int64.max")
        refused(RegionCommands.crossfade, ids(Int64.max, 30, 11), "lane", "seq at Int64.max")
        refused(
            RegionCommands.crossfade,
            [all[0], all[1], ControlArg(name: "event", value: .text(value: "11.5"))], "event", "text id")
        refused(
            RegionCommands.crossfade,
            [all[0], all[1], ControlArg(name: "event", value: .flag(value: true))], "event", "flag id")
        refused(RegionCommands.crossfade, ids(3, 31, 1), "event", "the note")
        refused(RegionCommands.crossfade, ids(3, 30, 999), "event", "unknown event")
        refused(RegionCommands.crossfade, ids(3, 30, 12), "event", "another track's region")
        refused(RegionCommands.crossfade, ids(3, 99, 11), "lane", "unknown track")
        refused(RegionCommands.crossfade, ids(-1, 30, 11), "seq", "negative")
        refused(RegionCommands.removeCrossfade, ids(3, 31, 1), "event", "remove on the note")
        let wrongType = [
            ControlArg(name: "seq", value: .text(value: "3")), ControlArg(name: "lane", value: .int(value: 30)),
            ControlArg(name: "event", value: .int(value: 11)),
        ]
        refused(RegionCommands.crossfade, wrongType, "seq", "text id")
        #expect(f.edits.isEmpty, "none of them reached the session's edit")
        // A, which starts inside nothing, reaches the core and fails with its sentence.
        f.editError = .Model(message: "that region does not start inside another")
        let out = RegionCommands.runChannel(RegionCommands.crossfade, args: ids(3, 30, 10), doc: UUID(), m, defaultMs: 10)
        guard case .failed(let fail) = out, fail.code == .failed else {
            Issue.record("\(out)")
            return
        }
        #expect(fail.message.contains("that region does not start inside another"))
        #expect(fail.message.contains("Crossfade Failed"))
    }

    @Test func crossfadeChannelCarriesTheSettleNotes() throws {
        let (m, f) = try crossfadeModel()
        pick(m, [11])
        nextSnapshot(f) { $0.editNotes = ["The crossfade was shortened to fit."] }
        let out = RegionCommands.runChannel(RegionCommands.crossfade, args: [], doc: UUID(), m, defaultMs: 10)
        guard case .done(let r) = out, case .ran(_, _, _, _, let status) = r else {
            Issue.record("\(out)")
            return
        }
        #expect(status?.contains("The crossfade was shortened to fit.") == true)
    }

    @Test func crossfadeIsUnavailableDuringAGesture() throws {
        let (m, f) = try crossfadeModel()
        pick(m, [11])
        #expect(m.editor.beginGesture("Drag", lanes: [], origin: .other))
        #expect(!RegionCommands.enabled(RegionCommands.crossfade, m))
        #expect(RegionCommands.crossfade(m, defaultMs: 10) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(RegionCommands.removeCrossfade(m) == .unavailable(EditorModel.gestureOpenMessage))
        let out = RegionCommands.runChannel(RegionCommands.crossfade, args: [], doc: UUID(), m, defaultMs: 10)
        guard case .failed(let fail) = out, fail.code == .unavailable else {
            Issue.record("\(out)")
            return
        }
        #expect(f.edits.isEmpty)
    }

    @Test func removeCrossfadeNeedsACrossfade() throws {
        let (m, f) = try crossfadeModel()
        pick(m, [10, 11])
        #expect(RegionCommands.enabled(RegionCommands.removeCrossfade, m))
        #expect(RegionCommands.removeCrossfade(m) == .done)
        #expect(f.edits == [.audio(edit: .removeCrossfade(event: 11))])
        // The later region has an overlap but no crossfade flag.
        var b = regionRow(11, tick: 30_720, duration: 61_440, frames: 96_000)
        b.overlapWith = 10
        b.overlapFrames = 24_000
        let a = regionRow(10, tick: 0, duration: 61_440, frames: 96_000)
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 9, events: [a, b])
        f.snap.laneRevs[0].rev = 9
        f.snap.version += 1
        m.apply(f.snap)
        #expect(!RegionCommands.enabled(RegionCommands.removeCrossfade, m))
        #expect(RegionCommands.removeCrossfade(m) == .unavailable("The selected region has no crossfade."))
        pick(m, [10])
        #expect(RegionCommands.removeCrossfade(m) == .unavailable("The selected region has no crossfade."))
        #expect(f.edits.count == 1)
    }

    @Test func crossfadeIdsNeverRunFadeTo() throws {
        let f = editorFake()
        let a = regionRow(10, tick: 0, duration: 61_440, frames: 96_000)
        var b = regionRow(11, tick: 30_720, duration: 61_440, frames: 96_000)
        b.overlapWith = 10
        b.overlapFrames = 24_000
        b.crossfade = true
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [a, b])
        let host = FakeControlHost()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let doc = try #require(s.newDemo())
        let dm = try #require(s.model(doc))
        dm.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 30)], events: [10, 11]))
        for id in [RegionCommands.crossfade, RegionCommands.removeCrossfade] {
            #expect(RegionCommands.ids.contains(id))
            #expect(ControlCommands.handled.contains(id))
            _ = RegionCommands.run(id, dm)
            _ = ControlCommands.run(id, args: [], doc: (doc, dm), store: s, host: host)
        }
        #expect(
            f.edits == [
                .audio(edit: .crossfade(event: 11, length: .upTo(frames: 480), shape: .equalPower)),
                .audio(edit: .crossfade(event: 11, length: .upTo(frames: 480), shape: .equalPower)),
                .audio(edit: .removeCrossfade(event: 11)),
                .audio(edit: .removeCrossfade(event: 11)),
            ])
        // No target: nothing is sent, and the sentence is the crossfade's.
        f.edits = []
        dm.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: []))
        for id in [RegionCommands.crossfade, RegionCommands.removeCrossfade] {
            #expect(!RegionCommands.enabled(id, dm))
            #expect(RegionCommands.run(id, dm) != .done)
            let out = ControlCommands.run(id, args: [], doc: (doc, dm), store: s, host: host)
            guard case .failed(let fail) = out, fail.code == .unavailable else {
                Issue.record("\(out)")
                return
            }
        }
        #expect(f.edits.isEmpty)
        // The default length comes from the host.
        host.crossfadeMs = 50
        dm.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 30)], events: [11]))
        _ = ControlCommands.run(RegionCommands.crossfade, args: [], doc: (doc, dm), store: s, host: host)
        #expect(f.edits.last == .audio(edit: .crossfade(event: 11, length: .upTo(frames: 2_400), shape: .equalPower)))
    }

    @Test func channelRemoveWithoutACrossfadeIsUnavailableBeforeTheCore() throws {
        let (m, f) = try crossfadeModel()
        var b = regionRow(11, tick: 30_720, duration: 61_440, frames: 96_000)
        b.overlapWith = 10
        b.overlapFrames = 24_000
        f.laneEventTable["3/30"] = LaneEvents(
            seq: 3, lane: 30, rev: 1, events: [regionRow(10, tick: 0, duration: 61_440, frames: 96_000), b])
        pick(m, [11])
        let out = RegionCommands.runChannel(RegionCommands.removeCrossfade, args: [], doc: UUID(), m, defaultMs: 10)
        guard case .failed(let fail) = out, fail.code == .unavailable else {
            Issue.record("\(out)")
            return
        }
        #expect(fail.message == "The selected region has no crossfade.")
        #expect(f.edits.isEmpty)
    }

    @Test func aBadDefaultLengthStillMakesOneFrame() throws {
        let (m, f) = try crossfadeModel()
        pick(m, [11])
        for ms in [Double.nan, 0, -4] {
            #expect(RegionCommands.crossfade(m, defaultMs: ms) == .done)
            #expect(f.edits.last == .audio(edit: .crossfade(event: 11, length: .upTo(frames: 1), shape: .equalPower)))
        }
    }
}
