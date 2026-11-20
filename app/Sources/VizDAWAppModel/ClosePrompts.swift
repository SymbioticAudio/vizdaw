import Foundation

/// The user's answer to a document's "save the changes?" sheet.
public enum CloseChoice: Sendable { case save, dontSave, cancel }

/// What a quit does first (release gate B1).
public enum QuitStep: Equatable, Sendable { case quitNow, askOne(UUID), askMany(count: Int), cancel }

/// Closing or quitting with unsaved changes asks first, the macOS way
/// (release gate B1, owner-approved). Only the user's own closes and quits
/// ask: control-channel closes and quits, Co-Producer and the app's own
/// quiet closes never do.
public enum ClosePrompts {
    /// What a quit does first, for the documents in window order.
    /// `testHooks` (`VIZDAW_TEST_HOOKS=1`, or a control-channel quit)
    /// quits at once, so the e2e never waits on a prompt.
    public static func quitStep(dirty: [UUID], testHooks: Bool) -> QuitStep {
        if testHooks || dirty.isEmpty { return .quitNow }
        return dirty.count == 1 ? .askOne(dirty[0]) : .askMany(count: dirty.count)
    }

    /// Message and informative text of a document's sheet.
    public static func sheetText(name: String) -> (message: String, informative: String) {
        ("Do you want to save the changes you made to \u{201C}\(name)\u{201D}?",
         "Your changes will be lost if you don't save them.")
    }

    /// The sheet while a Save made during recording is still to land
    /// (fix round 1, I-2): its informative text says the song will be
    /// saved, and it offers no Don't Save (closing lands the pass and
    /// saves the whole song).
    public static let savePendingInformative =
        "Recording is still going. The take and your changes will be saved when it stops."

    /// Message, informative text and whether Don't Save is offered, for a
    /// song whose save-after-pass is (`savePending`) or is not pending.
    public static func sheetText(name: String, savePending: Bool)
        -> (message: String, informative: String, offersDontSave: Bool)
    {
        let t = sheetText(name: name)
        return savePending ? (t.message, savePendingInformative, false) : (t.message, t.informative, true)
    }

    /// The quit alert's message with `count` (two or more) dirty documents.
    public static func quitAlertText(count: Int) -> String {
        "You have \(count) VizDAW documents with unsaved changes. Do you want to review these changes before quitting?"
    }

    /// The quit alert's message and informative text (next small fixes
    /// Task 6, R6): `discardable` are the songs Discard Changes loses,
    /// `savingAfterPass` those whose Save waits for their recording pass,
    /// which Discard Changes still saves. With none waiting the
    /// informative text is empty, as before.
    public static func quitAlertText(discardable: [String], savingAfterPass: [String])
        -> (message: String, info: String)
    {
        let message = quitAlertText(count: discardable.count + savingAfterPass.count)
        let quoted = savingAfterPass.map { "\u{201C}\($0)\u{201D}" }
        switch quoted.count {
        case 0:
            return (message, "")
        case 1:
            return (message, "If you discard changes, \(quoted[0]) will still be saved when its recording ends.")
        default:
            let names = quoted.dropLast().joined(separator: ", ") + " and " + quoted[quoted.count - 1]
            return (message, "If you discard changes, \(names) will still be saved when their recordings end.")
        }
    }

    /// `app.quit`'s refusal while songs have unsaved changes (next small
    /// fixes Task 6, R7): names them by their window titles.
    public static func quitRefusal(titles: [String]) -> String {
        let names = titles.map { "\u{201C}\($0)\u{201D}" }.joined(separator: ", ")
        let what = titles.count == 1 ? "1 song has" : "\(titles.count) songs have"
        return "\(what) unsaved changes: \(names). Save or close them first, or pass discard: true "
            + "(their changes are lost; a Save made while recording is still made)."
    }

    public static let save = "Save"
    public static let dontSave = "Don't Save"
    public static let cancel = "Cancel"
    public static let reviewChanges = "Review Changes\u{2026}"
    public static let discardChanges = "Discard Changes"
}

/// Quit › Review Changes…: the dirty documents one at a time. Save (when it
/// saved) and Don't Save close the song; Cancel, or a Save that did not save
/// (its panel cancelled, or the save failed), stops the quit. Choices already
/// made stand.
public struct QuitWalk: Sendable {
    /// Songs still open, in the order they are asked.
    public private(set) var open: [UUID]
    /// Songs answered and closed, in order.
    public private(set) var closed: [UUID] = []
    /// `.quitNow` once every song is answered, `.cancel` once stopped;
    /// nil while asking.
    public private(set) var result: QuitStep?

    public init(_ dirty: [UUID]) {
        open = dirty
        if dirty.isEmpty { result = .quitNow }
    }

    /// The song to ask about next (nil once finished).
    public var current: UUID? { result == nil ? open.first : nil }

    /// The current song cannot be asked (its window already shows a
    /// sheet, or it closed mid-walk): the quit stops, so it is always
    /// answered (fix round 1, I-1).
    public mutating func windowGone() {
        guard current != nil else { return }
        result = .cancel
    }

    /// The answer for `current`; `saved` is whether a `.save` saved.
    public mutating func answer(_ choice: CloseChoice, saved: Bool = false) {
        guard let id = current else { return }
        switch choice {
        case .save where !saved, .cancel:
            result = .cancel
        case .save, .dontSave:
            open.removeFirst()
            closed.append(id)
            if open.isEmpty { result = .quitNow }
        }
    }
}
