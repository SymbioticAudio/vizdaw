import VizDAWCore

/// A note an editor sends straight to a destination (spec §3.2).
public struct AuditionNote: Hashable, Sendable {
    public let dest: UInt64
    public let pitch: UInt8

    public init(dest: UInt64, pitch: UInt8) {
        self.dest = dest
        self.pitch = pitch
    }
}

/// Audition notes (Plan 4b Ruling 2): a failed note-on is dropped; a
/// failed note-off is retried by `tick()` so no note sticks.
extension EditorModel {
    /// Ticks of the 60 Hz plugin timer (about 2 s) a note-off is retried for.
    public static let noteOffRetries = 120
    public static let auditionHint = "Notes sound once the audio engine runs: press Play once."

    /// Sounds `note`. Note-offs still pending are retried first; while the
    /// one for this very note cannot be sent, the note is not started.
    public func auditionOn(_ note: AuditionNote, velocity: UInt16) {
        retryNoteOffs()
        guard pendingOffs[note] == nil else { return }
        do {
            let sent = try session.audition(dest: note.dest, pitch: note.pitch, velocity: velocity, on: true)
            if sent {
                heldAuditions.insert(note)
            } else if !auditionHintShown {
                auditionHintShown = true
                status = Self.auditionHint
            }
        } catch VizError.Busy {
            return
        } catch {
            report(error)
        }
    }

    /// Ends `note` if it sounds; a failed note-off is retried by `tick()`.
    public func auditionOff(_ note: AuditionNote) {
        guard heldAuditions.remove(note) != nil else { return }
        sendOff(note, attemptsLeft: Self.noteOffRetries)
    }

    /// Ends every audition note this document sounds.
    public func auditionOffAll() {
        for note in heldAuditions { auditionOff(note) }
    }

    public var hasPendingNoteOffs: Bool { !pendingOffs.isEmpty }

    /// One retry of every pending note-off.
    func retryNoteOffs() {
        for (note, left) in pendingOffs {
            pendingOffs[note] = nil
            sendOff(note, attemptsLeft: left - 1)
        }
    }

    private func sendOff(_ note: AuditionNote, attemptsLeft: Int) {
        do {
            _ = try session.audition(dest: note.dest, pitch: note.pitch, velocity: 0, on: false)
        } catch {
            if attemptsLeft > 0 { pendingOffs[note] = attemptsLeft }
        }
    }
}
