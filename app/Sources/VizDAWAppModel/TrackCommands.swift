import Foundation
import VizDAWCore

/// The Track menu's commands (plan tracks Ruling 5): New Audio, Instrument
/// and MIDI Track, Duplicate, Delete and Rename. Task 2 registers the IDs
/// and answers unavailable; Task 4 wires them.
@MainActor
public enum TrackCommands {
    public static let newAudio = "track.new_audio"
    public static let newInstrument = "track.new_instrument"
    public static let newMidi = "track.new_midi"
    public static let duplicate = "track.duplicate"
    public static let delete = "track.delete"
    public static let rename = "track.rename"
    public static let ids: Set<String> = [newAudio, newInstrument, newMidi, duplicate, delete, rename]

    /// The channel's entry: not wired yet.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        ControlCommands.unavailable("Track commands are not wired yet.")
    }
}
