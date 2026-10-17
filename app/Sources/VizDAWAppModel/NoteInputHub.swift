import Foundation
import VizDAWCore

/// Notes played on MIDI sources, handed to the documents whose list editors
/// listen (Plan 4c Ruling 11). The app's 60 Hz timer polls it. Input starts
/// when a list turns MIDI In on (stale notes are discarded and the sources
/// re-scanned) and stops when the last list turns it off; nothing is read
/// while no list listens. A failed start is retried only after `retryDelay`.
@MainActor
public final class NoteInputHub {
    private let input: (any NoteInputProtocol)?
    private let now: () -> Date
    private let retryDelay: TimeInterval
    private var started = false
    private var retryAt: Date?

    public init(
        input: (any NoteInputProtocol)?, retryDelay: TimeInterval = 5, now: @escaping () -> Date = Date.init
    ) {
        self.input = input
        self.retryDelay = retryDelay
        self.now = now
    }

    public var isListening: Bool { started }

    public func poll(_ docs: [DocumentModel]) {
        guard let input else { return }
        let listening = docs.filter(\.listensForNotes)
        if listening.isEmpty {
            retryAt = nil
            if started {
                started = false
                try? input.stop()
            }
            return
        }
        if !started {
            if let at = retryAt, now() < at { return }
            do {
                _ = try input.start()
                started = true
                retryAt = nil
                _ = try? input.take()  // notes played while nobody listened
            } catch {
                retryAt = now().addingTimeInterval(retryDelay)
            }
            return
        }
        guard let notes = try? input.take(), !notes.isEmpty else { return }
        for d in listening { d.receiveNotes(notes) }
    }
}
