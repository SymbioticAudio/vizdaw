import Foundation
import VizDAWCore

/// `lane.set_mpe` and `destination.set_mpe` (plan 5g-2 Task 7): a track's
/// MPE input or an external destination's MPE output, as one zone.
///
/// Errors: ids that are not ids, a missing or unknown `zone`, `members`
/// outside 1 to 15, `bend_range` outside 1 to 96, or either given with
/// `off` are `invalid_params` and never reach the session. Unknown ids and
/// the core's own refusals are `failed` with the core's message. An open
/// editor drag is `unavailable`. Nothing here opens a dialog or a window.
@MainActor
public enum MpeCommands {
    public static let setLane = "lane.set_mpe", setDestination = "destination.set_mpe"
    public static let ids: Set<String> = [setLane, setDestination]

    /// The setting `args` ask for: `.some(nil)` for off, nil when refused (zone missing or unknown, members or bend range out of range or given with off).
    public static func mpe(_ args: [ControlArg]) -> MpeRow?? {
        let members = args.int("members"), bend = args.int("bend_range")
        if args.contains(where: { $0.name == "members" }) && members == nil { return nil }
        if args.contains(where: { $0.name == "bend_range" }) && bend == nil { return nil }
        guard let zone = args.text("zone") else { return nil }
        let side: MpeZoneRow
        switch zone {
        case "off":
            return members == nil && bend == nil ? .some(nil) : nil
        case "lower": side = .lower
        case "upper": side = .upper
        default: return nil
        }
        if let members, !(1...15).contains(members) { return nil }
        if let bend, !(1...96).contains(bend) { return nil }
        return .some(MpeRow(zone: side, members: UInt8(members ?? 15), bendRange: UInt8(bend ?? 48)))
    }

    static func id(_ args: [ControlArg], _ name: String) -> UInt64? {
        args.int(name).flatMap { UInt64(exactly: $0) }
    }

    /// Runs `id` (one of `ids`) on document `doc`.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        let usage: String
        switch id {
        case setLane: usage = "lane.set_mpe: `seq` and `lane` name the track"
        case setDestination: usage = "destination.set_mpe: `dest` names an external MIDI destination"
        default: return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        let tail = "; `zone` is off, lower or upper, `members` 1 to 15 and `bend_range` 1 to 96 (neither with off)"
        let ids = id == setLane ? [Self.id(args, "seq"), Self.id(args, "lane")] : [Self.id(args, "dest")]
        guard !ids.contains(where: { $0 == nil }), let setting = mpe(args) else {
            return .failed(.of(.invalidParams, usage + tail))
        }
        do {
            if id == setLane, let seq = ids[0], let lane = ids[1] {
                m.apply(try m.session.edit(cmd: .lane(edit: .setMpe(seq: seq, lane: lane, mpe: setting)), filter: nil))
            } else if let dest = ids[0] {
                m.apply(try m.session.setDestinationMpe(dest: dest, mpe: setting))
            }
        } catch {
            return .failed(.thrown(error, "Set MPE Failed"))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
