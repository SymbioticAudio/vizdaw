import Foundation
import VizDAWCore

/// One entry of an MPE menu section; `mpe` nil is Off.
public struct MpeChoice: Equatable, Sendable {
    public let title: String
    public let mpe: MpeRow?
    public let checked: Bool
}

/// A titled section of the track header's MPE menus.
public struct MpeSection: Equatable, Sendable {
    public let title: String
    public let choices: [MpeChoice]
}

/// The MPE sections of the track header's input and destination menus (plan
/// 5g-2 Task 8; Ruling 11): the zone, and with MPE on the member channels
/// and the pitch bend range.
@MainActor
public enum MpeMenus {
    nonisolated static let defaultSetting = (members: UInt8(15), bendRange: UInt8(48))
    nonisolated static let bendRanges: [UInt8] = [2, 12, 24, 48, 96]

    /// "MPE Input": Off, Lower Zone, Upper Zone; with MPE on, "Member Channels" 1–15 and "Pitch Bend Range" 2, 12, 24, 48, 96 (Ruling 11). Empty unless the track's input is MIDI.
    nonisolated public static func input(_ lane: LaneRow) -> [MpeSection] {
        guard case .midi = lane.input else { return [] }
        return sections(title: "MPE Input", current: lane.mpe)
    }

    /// The same for destination `d`, titled "MPE Output: ‹name›".
    nonisolated public static func output(_ d: DestRow) -> [MpeSection] {
        sections(title: "MPE Output: \(d.name)", current: d.mpe)
    }

    nonisolated private static func sections(title: String, current: MpeRow?) -> [MpeSection] {
        let members = current?.members ?? defaultSetting.members
        let bend = current?.bendRange ?? defaultSetting.bendRange
        func zone(_ z: MpeZoneRow, _ name: String) -> MpeChoice {
            MpeChoice(title: name, mpe: MpeRow(zone: z, members: members, bendRange: bend), checked: current?.zone == z)
        }
        var out = [MpeSection(title: title, choices: [
            MpeChoice(title: "Off", mpe: nil, checked: current == nil),
            zone(.lower, "Lower Zone"), zone(.upper, "Upper Zone"),
        ])]
        if let current {
            out.append(MpeSection(title: "Member Channels", choices: (1...15).map { n in
                MpeChoice(
                    title: "\(n)", mpe: MpeRow(zone: current.zone, members: UInt8(n), bendRange: current.bendRange),
                    checked: current.members == UInt8(n))
            }))
            out.append(MpeSection(title: "Pitch Bend Range", choices: bendRanges.map { r in
                MpeChoice(
                    title: "\(r)", mpe: MpeRow(zone: current.zone, members: current.members, bendRange: r),
                    checked: current.bendRange == r)
            }))
        }
        return out
    }

    /// Sets the track's MPE input (one undo step).
    public static func chooseInput(_ c: MpeChoice, lane: LaneRow, _ m: DocumentModel) {
        guard let seq = m.selectedSeq, c.mpe != lane.mpe else { return }
        m.editor.edit(.lane(edit: .setMpe(seq: seq, lane: lane.id, mpe: c.mpe)))
    }

    /// Sets destination `dest`'s MPE output (one undo step).
    public static func chooseOutput(_ c: MpeChoice, dest: UInt64, _ m: DocumentModel) {
        m.editor.change { try m.session.setDestinationMpe(dest: dest, mpe: c.mpe) }
    }
}
