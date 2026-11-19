import Foundation
import Observation
import VizDAWCore

/// Settings › Co-Producer's "Notes and copies kept for N unsaved songs":
/// the stand-in folders `<support>/Co-Producer/Untitled/<id>/` of songs
/// that were never saved. A song open right now is never listed and never
/// deleted. The confirm is the composer's own click; nothing the control
/// channel does reaches it.
@MainActor
@Observable
public final class StandInFolders {
    /// The core's list; tests give their own.
    @ObservationIgnored public var list: @MainActor () throws -> [StandIn] = { try listStandIns() }
    /// The core's delete (with the open songs' IDs); tests give their own.
    @ObservationIgnored public var delete: @MainActor ([String]) throws -> UInt32 = {
        try deleteStandIns(openIds: $0)
    }
    /// The project IDs of the songs open right now.
    @ObservationIgnored public var openIDs: @MainActor () -> [String]
    /// The support folder, for Show in Finder.
    public let supportDir: URL

    /// What Settings lists: the stand-ins of no open song.
    public private(set) var rows: [StandIn] = []
    /// Delete… was clicked and the confirm shows.
    public var confirming = false
    /// "Could not …", after a failure.
    public private(set) var problem: String?

    public init(supportDir: URL, openIDs: @escaping @MainActor () -> [String]) {
        self.supportDir = supportDir
        self.openIDs = openIDs
    }

    /// The folder Show in Finder opens.
    public var folder: URL {
        supportDir.appendingPathComponent("Co-Producer").appendingPathComponent("Untitled")
    }

    /// Reads the folders again.
    public func refresh() {
        let open = Set(openIDs())
        do {
            rows = try list().filter { !open.contains($0.id) }
            problem = nil
        } catch {
            rows = []
            problem = "Could not read the notes and copies of unsaved songs: \(error.localizedDescription)"
        }
    }

    private static func songs(_ n: Int) -> String { n == 1 ? "1 unsaved song" : "\(n) unsaved songs" }

    /// "Notes and copies kept for N unsaved songs (X MB)"; nil, so the
    /// line is hidden, when there are none.
    public var line: String? {
        guard !rows.isEmpty else { return nil }
        let bytes = rows.reduce(into: Int64(0)) { $0 += Int64(clamping: $1.bytes) }
        let size = ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
        return "Notes and copies kept for \(Self.songs(rows.count)) (\(size))"
    }

    /// The confirm's question.
    public var confirmMessage: String {
        let n = rows.count
        return "Delete the notes and copies Co-Producer kept for \(n) \(n == 1 ? "song that was" : "songs that were") never saved?"
    }

    /// Delete: the open songs' folders stay.
    public func deleteAll() {
        confirming = false
        var failure: String?
        do {
            _ = try delete(openIDs())
        } catch {
            failure = "Could not delete the notes and copies: \(error.localizedDescription)"
        }
        refresh()
        if let failure { problem = failure }
    }
}
