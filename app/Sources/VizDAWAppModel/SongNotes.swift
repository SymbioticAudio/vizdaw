import Foundation
import VizDAWCore

/// Window › Song Notes (plan 8b Ruling 4): the song's `Notes.md`, made when
/// absent, opened in the composer's own editor. Over a channel the command
/// only answers the path: a programmatic call launches nothing.
@MainActor
public enum SongNotes {
    public static let command = "window.song_notes"

    /// Makes the notes file if needed and opens it with `open`; the alert
    /// text when it cannot.
    public static func open(_ m: DocumentModel, with open: (URL) -> Bool) -> String? {
        let path: String
        do {
            path = try m.session.notesPath()
        } catch {
            return "Song notes could not be opened: \(sentence(error))."
        }
        guard open(URL(fileURLWithPath: path)) else { return "No app opened Notes.md. It is at \(path)." }
        return nil
    }

    /// The menu item: `open`, and what went wrong as the window's alert.
    public static func openFromMenu(_ m: DocumentModel, with open: (URL) -> Bool) {
        if let text = Self.open(m, with: open) { m.alert = AlertItem(title: "Song Notes", message: text) }
    }

    /// The channel's `window.song_notes`: the path, made if needed; nothing
    /// is opened.
    static func run(_ docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        do {
            let path = try m.session.notesPath()
            return .done(.ran(command: command, doc: docID.uuidString, window: nil, undoLabel: m.snapshot.undoLabel, status: path))
        } catch {
            return .failed(CoProducerCommands.failure(error))
        }
    }

    /// The session's words without their closing full stop, to end a
    /// sentence of ours.
    static func sentence(_ error: any Error) -> String {
        let text = AlertItem(error, title: "Song Notes").message
        return text.hasSuffix(".") ? String(text.dropLast()) : text
    }
}
