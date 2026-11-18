import Foundation
@testable import VizDAWAppModel
import VizDAWCore

/// VizDAW's approval sheet without a window (plan 8b Task 6): records each
/// `ask` and `cancel`, and answers at once (`.allow`, `.deny`) or when the
/// test says so (`.wait`, then `answer`). `FakeApprover(.allow)` is the
/// test hook that grants VizDAW's approval as the sheet's Allow would.
@MainActor
final class FakeApprover: CoProducerApprover {
    enum Mode { case allow, deny, wait }
    let mode: Mode
    private(set) var asked: [PendingApproval] = []
    private(set) var cancelled: [UUID] = []
    private var done: (@MainActor (ApprovalAnswer) -> Void)?

    init(_ mode: Mode = .wait) { self.mode = mode }

    func ask(_ pending: PendingApproval, done: @escaping @MainActor (ApprovalAnswer) -> Void) {
        asked.append(pending)
        self.done = done
        switch mode {
        case .allow: done(.allow(always: false, confirmed: false))
        case .deny: done(.deny)
        case .wait: break
        }
    }

    /// The composer's answer on the open sheet, as a real press.
    func answer(_ a: ApprovalAnswer) { done?(a) }

    func cancel(doc: UUID) { cancelled.append(doc) }
}
