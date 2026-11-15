import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The DAWproject export sheet's model (plan 6d Task 12): the core's
/// check off the main actor (only the latest lands), the core's file name,
/// and an export that waits for a running job instead of failing as Busy.
@MainActor
@Suite(.serialized) struct DawprojectModelTests {
    func fake() -> FakeSession {
        let f = demoFake()
        f.dawprojectCheckReply = { ReportRow(summary: "check \($0.regionGain)", items: []) }
        return f
    }

    func shown(_ f: FakeSession) async throws -> (DocumentModel, DawprojectModel) {
        let m = try DocumentModel(session: f)
        m.dawproject.shown = true
        await m.dawproject.settle()
        return (m, m.dawproject)
    }

    /// A running job of `kind`, listed as `startJob` lists one.
    func running(_ f: FakeSession, _ m: DocumentModel, _ kind: JobKindRow, id: UInt64 = 99) {
        let row = JobRow(id: id, kind: kind, progress: 0.4, state: .running, message: nil, problems: [], created: [])
        f.jobRows[id] = row
        m.jobs.append(row)
    }

    func end(_ f: FakeSession, _ m: DocumentModel, _ id: UInt64, _ kind: JobKindRow, _ state: JobStateRow,
             message: String? = nil, report: ReportRow? = nil) {
        f.jobRows[id] = JobRow(
            id: id, kind: kind, progress: 1, state: state, message: message, problems: [], created: [],
            report: report, written: report == nil ? [] : ["/tmp/a.dawproject"])
        m.pollJobs()
    }

    @Test func checkRunsDetachedAndKeepsTheLatest() async throws {
        let f = fake()
        let (m, d) = try await shown(f)
        defer { withExtendedLifetime(m) {} }
        #expect(f.dawprojectChecks.count == 1 && f.dawprojectChecks[0].path == "")
        #expect(d.wontTransfer?.summary == "check apply" && d.checkError == nil)
        #expect(f.checkOnMain == [false], "the check ran in a detached task")

        // Two quick changes: the first is held, the second's answer wins.
        let release = DispatchSemaphore(value: 0)
        f.dawprojectCheckHold = { $0.regionGain == .leaveOut ? release : nil }
        f.dawprojectCheckReply = { ReportRow(summary: "second \($0.regionGain)", items: []) }
        d.regionGain = .leaveOut
        d.regionGain = .apply
        for _ in 0..<500 where d.wontTransfer?.summary != "second apply" { try await Task.sleep(for: .milliseconds(10)) }
        #expect(d.wontTransfer?.summary == "second apply")
        release.signal()
        await d.settle()
        #expect(d.wontTransfer?.summary == "second apply", "a stale answer is dropped")
        #expect(f.dawprojectChecks.count == 3 && !f.checkOnMain.contains(true))

        // The core's refusal is the sheet's message.
        f.dawprojectCheckHold = nil
        f.dawprojectCheckReply = nil
        f.dawprojectCheckAnswer = .failure(VizError.Model(message: "no such sequence"))
        d.refresh()
        await d.settle()
        #expect(d.wontTransfer == nil && d.checkError == "no such sequence")
    }

    @Test func defaultNameComesFromTheCore() async throws {
        let f = fake()
        f.dawprojectNameAnswer = "Odd · Name.dawproject"
        let (m, d) = try await shown(f)
        #expect(d.defaultName == "Odd · Name.dawproject", "shown as the core gave it")
        #expect(f.dawprojectNames == [try #require(m.selectedSeq)])
        // Opening again asks again.
        d.shown = false
        f.dawprojectNameAnswer = "Song.dawproject"
        d.shown = true
        await d.settle()
        #expect(d.defaultName == "Song.dawproject" && f.dawprojectNames.count == 2)
        #expect(f.dawprojectChecks.count == 2, "opening refreshes the check")
    }

    @Test func exportWaitsForTheRunningJob() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        running(f, m, .bounce)
        d.start(path: "/tmp/a.dawproject", replace: true)
        #expect(d.waiting && f.dawprojectExports.isEmpty && m.alert == nil && d.job == nil)
        m.pollJobs()
        #expect(f.dawprojectExports.isEmpty, "still running")
        end(f, m, 99, .bounce, .done)
        #expect(f.dawprojectExports.count == 1 && !d.waiting && d.running)
        let r = try #require(f.dawprojectExports.last)
        #expect(r.path == "/tmp/a.dawproject" && r.replace && r.seq == m.selectedSeq)
    }

    /// Controller ruling: the core refuses an export as Busy beside the
    /// media jobs too, which `ForDawModel.blocking` does not list.
    @Test func exportWaitsForAMediaJob() async throws {
        for kind: JobKindRow in [.format, .convert, .relink, .externalEdit] {
            let f = fake()
            f.jobStartState = .running
            let (m, d) = try await shown(f)
            running(f, m, kind)
            d.start(path: "/tmp/a.dawproject", replace: false)
            #expect(d.waiting && f.dawprojectExports.isEmpty && m.alert == nil, "\(kind)")
            end(f, m, 99, kind, .done)
            #expect(f.dawprojectExports.count == 1 && !d.waiting, "\(kind)")
            #expect(f.dawprojectExports.last?.replace == false)
        }
    }

    /// Fix round 1: the panel confirmed a replace only of what was there
    /// when it returned. A file that appears while the export waits is not
    /// replaced: the request goes without `replace`, and the core refuses.
    @Test func replaceOnlyWhatWasThereAtConfirm() async throws {
        for taken in [false, true] {
            let f = fake()
            f.jobStartState = .running
            let (m, d) = try await shown(f)
            var asked: [String] = []
            d.exists = { asked.append($0); return taken }
            running(f, m, .bounce)
            d.confirmed(path: "/tmp/a.dawproject")
            #expect(asked == ["/tmp/a.dawproject"], "checked once, at confirm")
            #expect(d.waiting && f.dawprojectExports.isEmpty)
            // Whatever is there by the time the bounce ends is not asked about.
            d.exists = { _ in Issue.record("checked again after confirm"); return true }
            end(f, m, 99, .bounce, .done)
            let r = try #require(f.dawprojectExports.last)
            #expect(r.path == "/tmp/a.dawproject" && r.replace == taken, "taken: \(taken)")
        }
    }

    @Test func cancelWaitingSendsNothing() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        running(f, m, .importMidi)
        d.start(path: "/tmp/a.dawproject", replace: false)
        #expect(d.waiting)
        d.cancelWaiting()
        #expect(!d.waiting)
        end(f, m, 99, .importMidi, .done)
        #expect(f.dawprojectExports.isEmpty && d.job == nil && !d.running)
    }

    @Test func failedJobShowsItsMessage() async throws {
        let f = fake()
        f.jobStartState = .running
        let (m, d) = try await shown(f)
        d.start(path: "/tmp/a.dawproject", replace: false)
        let job = try #require(d.job)
        #expect(d.running && m.jobs.contains { $0.id == job && $0.kind == .exportDawproject })
        d.shown = false
        end(f, m, job, .exportDawproject, .failed, message: "disk full")
        #expect(d.checkError == "disk full" && d.shown, "the sheet again, with the message")
        #expect(!m.interchange.reportShown && m.interchange.report == nil && m.alert == nil)
        #expect(!d.running)

        // A cancelled one is quiet; a finished one shows its report.
        d.start(path: "/tmp/b.dawproject", replace: false)
        let second = try #require(d.job)
        end(f, m, second, .exportDawproject, .cancelled)
        #expect(m.alert == nil && !m.interchange.reportShown && !d.shown)
        d.shown = true
        await d.settle()
        d.start(path: "/tmp/c.dawproject", replace: false)
        let third = try #require(d.job)
        end(f, m, third, .exportDawproject, .done, report: ReportRow(summary: "Exported 2 tracks", items: []))
        #expect(m.interchange.reportShown && m.interchange.report?.summary.hasPrefix("Exported") == true)
        #expect(m.interchange.written == ["/tmp/a.dawproject"] && !d.shown && m.alert == nil)

        // Refused when it starts: the message in the sheet, no alert.
        f.interchangeError = VizError.Model(message: "a.dawproject exists")
        d.shown = true
        await d.settle()
        d.start(path: "/tmp/a.dawproject", replace: false)
        #expect(d.checkError == "a.dawproject exists" && d.shown && m.alert == nil && !d.running)

        // A check still in flight when the start is refused lands late: the
        // refusal stays (fix round 1).
        let release = DispatchSemaphore(value: 0)
        f.dawprojectCheckHold = { $0.regionGain == .leaveOut ? release : nil }
        d.regionGain = .leaveOut
        d.start(path: "/tmp/a.dawproject", replace: false)
        #expect(d.checkError == "a.dawproject exists")
        release.signal()
        await d.settle()
        #expect(d.checkError == "a.dawproject exists", "the late check does not erase it")
    }

    @Test func requestCarriesTheBlockRule() async throws {
        let f = fake()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.editor.updateLayout { $0.blockRule = .fixedBars(2) }
        let d = m.dawproject
        d.shown = true
        await d.settle()
        let rule = BlockRuleSetting.fixedBars(2).rule
        #expect(f.dawprojectChecks.last?.blocks == rule)
        d.regionGain = .leaveOut
        await d.settle()
        #expect(f.dawprojectChecks.last?.regionGain == .leaveOut)
        d.start(path: "/tmp/a.dawproject", replace: true)
        #expect(
            f.dawprojectExports.last
                == DawprojectExportRequestRow(
                    seq: try #require(m.selectedSeq), path: "/tmp/a.dawproject", replace: true, regionGain: .leaveOut,
                    blocks: rule))
    }

    @Test func noSequenceIsTheSheetsMessage() async throws {
        let m = try DocumentModel(session: FakeSession([]))
        let d = m.dawproject
        d.shown = true
        await d.settle()
        #expect(d.checkError == "No sequence is shown." && d.wontTransfer == nil)
        d.start(path: "/tmp/a.dawproject", replace: false)
        #expect(d.job == nil && d.checkError == "No sequence is shown.")
    }
}
