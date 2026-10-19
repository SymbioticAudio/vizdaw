import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Import Audio, the project format and the banners (plan 5a Task 13).
@MainActor
struct ProjectMediaTests {
    func model(_ fake: FakeSession) throws -> DocumentModel {
        try DocumentModel(session: fake)
    }

    func fake() -> FakeSession {
        let f = FakeSession([seq(1, "Song", topLevel: true)])
        f.laneTable[1] = []
        return f
    }

    @Test func importTargetUsesTheSelectedLaneForOneFile() {
        let sel = [LaneRef(seq: 2, lane: 9), LaneRef(seq: 1, lane: 5)]
        #expect(DocumentModel.importTarget(seq: 1, selection: sel, files: 1, tick: 960)
            == ImportTargetRow(seq: 1, lane: 5, tick: 960))
        #expect(DocumentModel.importTarget(seq: 1, selection: sel, files: 2, tick: 960)
            == ImportTargetRow(seq: 1, lane: nil, tick: 960))
        #expect(DocumentModel.importTarget(seq: 1, selection: [], files: 1, tick: -100)
            == ImportTargetRow(seq: 1, lane: nil, tick: 0))
        #expect(DocumentModel.importTarget(seq: nil, selection: sel, files: 1, tick: 0) == nil)
    }

    @Test func importRunsAsAJobAndRefreshesWhenItEnds() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"])
        #expect(f.imports.count == 1)
        #expect(m.banners.contains { if case .job = $0 { true } else { false } })
        let before = m.mediaVersion
        f.jobRows[1] = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil, problems: [], created: [3])
        m.pollJobs()
        #expect(m.mediaVersion == before + 1)
        #expect(!m.banners.contains { if case .job = $0 { true } else { false } })
    }

    @Test func aFailedJobShowsAnAlertAndProblemsShowABanner() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav", "/b.wav"])
        f.jobRows[1] = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil,
                              problems: ["b.wav: not audio"], created: [7])
        m.pollJobs()
        #expect(m.banners.contains(.problems(["b.wav: not audio"])))
        m.dismissProblems()
        #expect(!m.banners.contains(.problems(["b.wav: not audio"])))
        m.importAudio(paths: ["/c.wav"])
        f.jobRows[2] = JobRow(id: 2, kind: .import, progress: 1, state: .failed, message: "disk full",
                              problems: [], created: [])
        m.pollJobs()
        #expect(m.alert?.message.contains("disk full") == true)
    }

    @Test func aJobThatCreatedNothingShowsAnAlertListingTheProblems() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.txt"])
        f.jobRows[1] = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil,
                              problems: ["a.txt: not audio"], created: [])
        m.pollJobs()
        #expect(m.alert?.message.contains("a.txt: not audio") == true)
        #expect(!m.banners.contains(.problems(["a.txt: not audio"])))
    }

    @Test func mismatchedMediaOffersConvert() throws {
        let f = fake()
        f.info.mismatched = 3
        let m = try model(f)
        m.refreshProjectInfo()
        #expect(m.banners.contains(.mismatched(count: 3, label: "48 kHz, 24-bit")))
        m.convertMedia()
        #expect(f.conversions == 1)
        m.pollJobs()
        #expect(!m.banners.contains { if case .mismatched = $0 { true } else { false } })
    }

    @Test func aDeviceAtAnotherRateOffersToChangeTheProjectRate() throws {
        let f = fake()
        f.info.rateMismatch = true
        f.info.deviceRate = 44_100
        let m = try model(f)
        m.refreshProjectInfo()
        #expect(m.banners.contains(.rateMismatch(device: 44_100, project: 48_000)))
        m.changeRateToDevice()
        #expect(f.formatChanges == [ProjectFormatRow(sampleRate: 44_100, depth: .int24)])
    }

    @Test func diskTooSlowFollowsTheFaults() throws {
        let f = fake()
        let m = try model(f)
        f.state.faults = ["stream-underrun"]
        m.poll()
        #expect(m.banners.contains(.diskTooSlow))
        m.dismissDiskWarning()
        #expect(!m.banners.contains(.diskTooSlow))
        // Cleared at the next Play: a new underrun shows again.
        f.state.faults = []
        m.poll()
        f.state.faults = ["stream-underrun"]
        m.poll()
        #expect(m.banners.contains(.diskTooSlow))
    }

    @Test func cancellingAJobAsksTheSession() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"])
        m.pollJobs()
        #expect(m.banners.contains(.job(id: 1, kind: .import, progress: 0.4)))
        m.cancelJob(1)
        #expect(f.cancelled == [1])
    }

    @Test func theDefaultFormatIsRemembered() {
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { FakeSession([]) }, open: { _ in FakeSession([]) })
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 48_000, depth: .int24))
        store.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .float32)
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 96_000, depth: .float32))
        store.defaults.set("garbage", forKey: "defaultProjectFormat")
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 48_000, depth: .int24))
    }
}
