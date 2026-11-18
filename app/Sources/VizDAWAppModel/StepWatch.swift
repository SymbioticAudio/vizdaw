import Foundation

/// What a document's new snapshot did to its history (plan 8f Ruling 4).
public enum StepEvent: Equatable, Sendable {
    /// A new step, with its undo label ("" when it has none).
    case newStep(label: String)
    case undo, redo
}

/// Tells steps, undos and redos apart from snapshots' `step` serials
/// (plan 8f Ruling 4): a step above the highest seen is new, one below the
/// last is an undo, and one back up to at most the highest is a redo. Two
/// steps between snapshots count as one.
public struct StepWatch: Sendable {
    private var last: UInt64?
    private var highest: UInt64 = 0

    public init() {}

    /// The event `step` (with `undoLabel`) makes; nil for the first
    /// snapshot and for one at the same step.
    public mutating func observe(step: UInt64, undoLabel: String?) -> StepEvent? {
        guard let last else {
            self.last = step
            highest = step
            return nil
        }
        guard step != last else { return nil }
        self.last = step
        if step > highest {
            highest = step
            return .newStep(label: undoLabel ?? "")
        }
        return step < last ? .undo : .redo
    }
}
