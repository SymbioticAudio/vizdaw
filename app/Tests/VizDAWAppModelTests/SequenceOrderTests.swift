import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct SequenceOrderTests {
    let rows = [
        seq(1, "Beat", topLevel: false), seq(2, "Groove", topLevel: false),
        seq(3, "Song", topLevel: true),
    ]

    @Test func listedFirstThenTheRestByID() {
        #expect(SequenceOrder.sorted(rows, by: [3]).map(\.id) == [3, 1, 2])
        #expect(SequenceOrder.sorted(rows, by: []).map(\.id) == [1, 2, 3])
        #expect(SequenceOrder.sorted(rows, by: [99, 2]).map(\.id) == [2, 1, 3], "stale IDs are ignored")
    }

    @Test func movingWithinASectionKeepsTheOthersInPlace() {
        let order = SequenceOrder.moved(
            all: rows, order: [], visible: [1, 2], from: IndexSet(integer: 1), to: 0)
        #expect(order == [2, 1, 3])
        let back = SequenceOrder.moved(
            all: rows, order: order, visible: [2, 1], from: IndexSet(integer: 0), to: 2)
        #expect(back == [1, 2, 3])
    }
}
