import Foundation
import Testing
import VizDAWAppModel
import VizDAWCore

@Suite struct ControlPolicyTests {
    @Test func noAlertSheetWhileTheChannelIsOn() {
        let a = AlertItem(title: "Mixer Unavailable", message: "x")
        #expect(ControlAlertLog.showsSheet(a, channelOn: false))
        #expect(!ControlAlertLog.showsSheet(a, channelOn: true))
        #expect(!ControlAlertLog.showsSheet(nil, channelOn: false))
    }

    @Test func eachDocumentAlertIsReportedOnce() {
        var log = ControlAlertLog()
        let a = AlertItem(title: "Transport Failed", message: "a")
        let b = AlertItem(title: "Mixer Unavailable", message: "b")
        #expect(log.unreported([a]) == [a])
        #expect(log.unreported([a]).isEmpty, "still showing: not again")
        #expect(log.unreported([a, b]) == [b])
        #expect(log.unreported([a, b]).isEmpty)
        #expect(log.unreported([]).isEmpty)
        // A new alert with the same text is a new alert.
        let a2 = AlertItem(title: "Transport Failed", message: "a")
        #expect(log.unreported([a2]).map(\.id) == [a2.id])
        // Two documents showing one alert item report it once.
        var log2 = ControlAlertLog()
        #expect(log2.unreported([a, a]).map(\.id) == [a.id])
    }

    @Test func aStaleWindowNumberIsNoWindow() {
        let exists: (Int64) -> Bool = { $0 == 7 }
        #expect(ControlWindowCheck.namesNoWindow(.number(number: 8), exists: exists))
        #expect(!ControlWindowCheck.namesNoWindow(.number(number: 7), exists: exists))
        #expect(!ControlWindowCheck.namesNoWindow(nil, exists: exists))
        #expect(!ControlWindowCheck.namesNoWindow(.key, exists: exists))
        #expect(!ControlWindowCheck.namesNoWindow(.main, exists: exists))
    }

    @Test func onlyDocumentWaitsReadTheirWindow() {
        #expect(ControlWindowCheck.waitReadsDocument(.playing))
        #expect(ControlWindowCheck.waitReadsDocument(.stopped))
        #expect(ControlWindowCheck.waitReadsDocument(.tickAtLeast(tick: 1)))
        #expect(ControlWindowCheck.waitReadsDocument(.detailKind(kind: "list")))
        #expect(!ControlWindowCheck.waitReadsDocument(.documentsAtLeast(count: 1)))
        #expect(!ControlWindowCheck.waitReadsDocument(.windowTitled(title: "x")))
        #expect(!ControlWindowCheck.waitReadsDocument(.windowGone(title: "x")))
    }
}
