import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Bounce sheet's model (plan 5f-2 Task 6; Rulings 3, 8, 11; owner
/// answer 2) and the tail setting (pre-flight I10).
@MainActor
@Suite(.serialized) struct BounceModelTests {
    func model(_ f: FakeSession) throws -> DocumentModel { try DocumentModel(session: f) }

    @Test func defaultsFollowTheProject() throws {
        let f = demoFake()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        let m = try model(f)
        m.bounce.shown = true
        let b = m.bounce
        #expect(b.source == .mix && b.container == .wav && b.rate == 44_100 && b.depth == .int16 && b.dither)
        #expect(!b.addToProject && !b.useSelection)
        b.depth = .int24
        #expect(b.dither, "kept as set")
        #expect(b.request(path: "/tmp/x.wav", replace: false)?.dither == true)
        b.depth = .float32
        #expect(b.request(path: "/tmp/x.wav", replace: false)?.dither == false, "float ignores dither")
        #expect(BounceModel.rates == [44_100, 48_000, 88_200, 96_000, 176_400, 192_000])
        // Showing again starts from the project's defaults.
        f.info.format = ProjectFormatRow(sampleRate: 96_000, depth: .int24)
        m.bounce.shown = false
        m.bounce.shown = true
        #expect(m.bounce.rate == 96_000 && m.bounce.depth == .int24 && !m.bounce.dither)
    }

    @Test func theWarningComesFromTheCore() throws {
        let f = demoFake()
        f.checkRow = BounceCheckRow(
            externalLanes: 1, frames: 96_000, files: [],
            warning: "1 lane plays external MIDI and will be silent. Track it as audio first: Track Synth…")
        let m = try model(f)
        m.bounce.shown = true
        #expect(m.bounce.warning == f.checkRow.warning)
        let asked = f.checkRequests.count
        m.bounce.source = .stems
        m.bounce.stems = [1]
        #expect(f.checkRequests.count > asked, "refetched after a change")
        #expect(f.checkRequests.last?.source == .stems(strips: [1]))
    }

    @Test func startBuildsTheRequest() throws {
        let f = demoFake()
        f.jobStartState = .running
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.source = .stems
        m.bounce.stems = [2, 1]
        m.bounce.start(path: "/tmp/stems", replace: false)
        let r = try #require(f.bounces.last)
        #expect(r.source == .stems(strips: [1, 2]) && r.path == "/tmp/stems" && !r.addToProject)
        #expect(m.bounce.job != nil && m.jobs.contains { $0.kind == .bounce })
        m.bounce.cancel()
        #expect(f.cancelled.last == m.bounce.job)
        m.bounce.source = .mix
        m.bounce.addToProject = true
        m.bounce.start(path: nil, replace: false)
        let add = try #require(f.bounces.last)
        #expect(add.path == nil && add.addToProject && add.source == .mix)
    }

    @Test func aRefusedStartIsAnAlert() throws {
        let f = demoFake()
        f.bounceError = VizError.Model(message: "x.wav exists; choose another name or replace it")
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.start(path: "/tmp/x.wav", replace: false)
        #expect(m.alert?.title == "Bounce Failed")
        #expect(m.bounce.job == nil)
    }

    /// Owner answer 2: stems never replace; the sheet shows the numbered
    /// names the core will take, and asks nothing.
    @Test func existingStemFilesAreNumberedNotReplaced() throws {
        let f = demoFake()
        f.checkRow = BounceCheckRow(externalLanes: 0, frames: 96_000, files: ["/tmp/s/Bass 2.wav"], warning: nil)
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.source = .stems
        m.bounce.stems = [1]
        m.bounce.check(path: "/tmp/s")
        #expect(m.bounce.files == ["/tmp/s/Bass 2.wav"])
        #expect(f.checkRequests.last?.path == "/tmp/s")
        m.bounce.start(path: "/tmp/s", replace: true)
        #expect(f.bounces.last?.replace == false, "stems never replace")
    }

    /// Pre-flight I10: kept, clamped, applied on change and at launch.
    @Test func tailSettingIsKeptAndApplied() throws {
        let defaults = MemoryDefaults()
        let sink = FakeAudioSink()
        let p = AudioPreferences(defaults: defaults, sink: sink)
        #expect(p.settings.bounceTailSeconds == 10)
        p.setBounceTail(seconds: 4)
        #expect(sink.calls.last == "bounce tail 4000" && p.settings.bounceTailSeconds == 4)
        p.setBounceTail(seconds: 99)
        #expect(sink.calls.last == "bounce tail 30000" && p.settings.bounceTailSeconds == 30)
        let again = FakeAudioSink()
        let q = AudioPreferences(defaults: defaults, sink: again)
        #expect(q.settings.bounceTailSeconds == 30, "read back from the preferences")
        q.applyAll()
        #expect(again.calls.contains("bounce tail 30000"), "applied at launch")
        // A damaged value keeps the default.
        let bad = MemoryDefaults()
        bad.set(Data(#"{"bounceTailSeconds": 999}"#.utf8), forKey: AudioPreferences.key)
        #expect(AudioPreferences(defaults: bad, sink: FakeAudioSink()).settings.bounceTailSeconds == 10)
    }
}
