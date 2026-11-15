import Foundation
import VizDAWCore

/// The Takes menu's commands (plan 5c-2 takes Rulings 5 and 6): choose
/// which take plays, rename a take and delete one. One implementation for
/// the menu and the control channel's `command.run`; every refusal is made
/// here, before any edit, and none opens a dialog. Only the menu's
/// Rename… asks for a name, through `DocumentModel.askTakeName`.
@MainActor
public enum TakeCommands {
    public static let choose = "track.choose_take"
    public static let rename = "track.rename_take"
    public static let delete = "track.delete_take"
    public static let ids: Set<String> = [choose, rename, delete]

    /// The core's refusal for the only take, in the app's words.
    public static let onlyTake = "A track keeps at least one take."
    /// Rename… with no document window to ask in.
    public static let noAsk = "Show the document window to rename a take."
    /// A blank name, or one holding a control character.
    static let badName = "A take name cannot be empty or hold control characters."

    /// "There is no take `take` on this track."
    static func noTake(_ take: UInt64) -> String { "There is no take \(take) on this track." }

    /// Whether `name` can name a take: not blank, and no control character
    /// (a NUL among them). The core checks it again.
    static func validName(_ name: String) -> Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !name.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }

    /// Why `id` cannot act on take `take` of track `row`, as the channel's
    /// failure; nil when it can.
    static func refusal(_ id: String, take: UInt64, of row: LaneRow) -> ControlFailure? {
        guard row.takes.contains(where: { $0.id == take }) else { return .of(.unknownId, noTake(take)) }
        if id == delete, row.takes.count < 2 { return .of(.failed, onlyTake) }
        return nil
    }

    // MARK: The menu's entries

    /// Plays take `take` of track `lane`.
    @discardableResult
    public static func choose(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel) -> KeyOutcome {
        fromMenu(choose, seq: seq, lane: lane, take: take, name: nil, m)
    }

    /// Renames take `take` of track `lane` to `name`.
    @discardableResult
    public static func rename(seq: UInt64, lane: UInt64, take: UInt64, to name: String, _ m: DocumentModel)
        -> KeyOutcome
    {
        fromMenu(rename, seq: seq, lane: lane, take: take, name: name, m)
    }

    /// Deletes take `take` of track `lane` (one undo step).
    @discardableResult
    public static func delete(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel) -> KeyOutcome {
        fromMenu(delete, seq: seq, lane: lane, take: take, name: nil, m)
    }

    /// The menu's Rename…: asks through `m.askTakeName`, then renames. No
    /// hook: the status line says why. Cancelled: nothing happens.
    public static func renameAsking(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel) {
        if let refused = TrackCommands.waitRefusal(m) {
            if case .unavailable(let why) = refused { m.editor.status = why }
            return
        }
        guard let row = track(seq: seq, lane: lane, m),
            let current = row.takes.first(where: { $0.id == take })
        else {
            m.editor.status = noTake(take)
            return
        }
        guard let ask = m.askTakeName else {
            m.editor.status = noAsk
            return
        }
        guard let name = ask(current.name) else { return }
        rename(seq: seq, lane: lane, take: take, to: name, m)
    }

    /// A menu entry: its refusals show in the status line.
    private static func fromMenu(
        _ id: String, seq: UInt64, lane: UInt64, take: UInt64, name: String?, _ m: DocumentModel
    ) -> KeyOutcome {
        let outcome = checked(id, seq: seq, lane: lane, take: take, name: name, m)
        switch outcome {
        case .done, .busy: break
        case .unavailable(let why), .refused(let why): m.editor.status = why
        }
        return outcome
    }

    /// Every refusal, then the edit.
    private static func checked(
        _ id: String, seq: UInt64, lane: UInt64, take: UInt64, name: String?, _ m: DocumentModel
    ) -> KeyOutcome {
        guard let row = track(seq: seq, lane: lane, m) else {
            return .refused("There is no track \(lane) in this sequence.")
        }
        if let f = refusal(id, take: take, of: row) { return .refused(f.message) }
        if let name, !validName(name) { return .refused(badName) }
        return execute(id, seq: seq, lane: lane, take: take, name: name, m)
    }

    /// The edit itself, after the refusals; the core's Busy stays Busy.
    private static func execute(
        _ id: String, seq: UInt64, lane: UInt64, take: UInt64, name: String?, _ m: DocumentModel
    ) -> KeyOutcome {
        if let refused = TrackCommands.waitRefusal(m) { return refused }
        let edit: LaneEdit
        switch id {
        case choose: edit = .setActiveTake(seq: seq, lane: lane, take: take)
        case rename: edit = .renameTake(seq: seq, lane: lane, take: take, name: name ?? "")
        default: edit = .removeTake(seq: seq, lane: lane, take: take)
        }
        if m.editor.edit(.lane(edit: edit)) { return .done }
        let why = m.editor.status ?? "The take was not changed."
        return why == TrackCommands.recordingBusy ? .busy(why) : .refused(why)
    }

    /// Track `lane` of sequence `seq`, as the menus show it; nil when there
    /// is none.
    public static func track(seq: UInt64, lane: UInt64, _ m: DocumentModel) -> LaneRow? {
        let rows = seq == m.selectedSeq ? m.lanes : ((try? m.session.lanes(seq: seq)) ?? [])
        return rows.first { $0.id == lane }
    }

    // MARK: The control channel

    /// The channel's entry: `seq` and `lane` default as `track.rename`'s
    /// do, `take` is required, and `rename_take` needs `name`. It answers
    /// `status` with the take's name: the chosen one's, the new name, or
    /// the deleted one's.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard ids.contains(id) else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        let seqArg = TrackCommands.number(args, "seq"), laneArg = TrackCommands.number(args, "lane")
        let takeArg = TrackCommands.number(args, "take")
        if seqArg.bad { return TrackCommands.badNumber(id, "seq") }
        if laneArg.bad { return TrackCommands.badNumber(id, "lane") }
        guard let take = takeArg.value else {
            if takeArg.bad { return TrackCommands.badNumber(id, "take") }
            return .failed(.of(.invalidParams, "\(id): missing `take`"))
        }
        var name: String?
        if id == rename {
            guard let n = args.text("name") else { return .failed(.of(.invalidParams, "\(id): `name` is text")) }
            guard validName(n) else { return .failed(.of(.invalidParams, "\(id): \(badName)")) }
            name = n.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        let t = TrackCommands.target(seq: seqArg.value, lane: laneArg.value, m)
        if let refusal = t.refusal { return refusal }
        guard let row = t.row else { return ControlCommands.unavailable(TrackCommands.noTrack) }
        if let f = refusal(id, take: take, of: row) { return .failed(f) }
        let before = row.takes.first { $0.id == take }?.name ?? ""
        let outcome = execute(id, seq: t.seq, lane: row.id, take: take, name: name, m)
        if let f = ControlCommands.failure(outcome) { return f }
        return .done(
            .ran(
                command: id, doc: doc.uuidString, window: nil, undoLabel: m.snapshot.undoLabel,
                status: name ?? before))
    }
}
