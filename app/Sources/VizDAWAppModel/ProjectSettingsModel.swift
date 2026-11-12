import Foundation
import Observation
import VizDAWCore

/// The 13 song-info texts of Project Settings; an empty text is no value.
public struct SongText: Equatable, Sendable {
    public var title = "", artist = "", album = "", originalArtist = "", composer = ""
    public var songwriter = "", producer = "", arranger = "", year = "", genre = ""
    public var copyright = "", website = "", comment = ""

    public init() {}

    public init(_ r: SongInfoRow) {
        title = r.title ?? ""
        artist = r.artist ?? ""
        album = r.album ?? ""
        originalArtist = r.originalArtist ?? ""
        composer = r.composer ?? ""
        songwriter = r.songwriter ?? ""
        producer = r.producer ?? ""
        arranger = r.arranger ?? ""
        year = r.year ?? ""
        genre = r.genre ?? ""
        copyright = r.copyright ?? ""
        website = r.website ?? ""
        comment = r.comment ?? ""
    }

    public var row: SongInfoRow {
        func v(_ s: String) -> String? { s.isEmpty ? nil : s }
        return SongInfoRow(
            title: v(title), artist: v(artist), album: v(album), originalArtist: v(originalArtist),
            composer: v(composer), songwriter: v(songwriter), producer: v(producer), arranger: v(arranger),
            year: v(year), genre: v(genre), copyright: v(copyright), website: v(website), comment: v(comment))
    }
}

/// Project Settings (plan 6a Task 9): the project format and the song info.
/// `apply()` sends only what changed.
@Observable
@MainActor
public final class ProjectSettingsModel {
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private var loadedSong = SongText()
    @ObservationIgnored private var loadedFormat = ProjectFormatRow(sampleRate: 48_000, depth: .int24)

    public var rate: UInt32 = 48_000
    public var depth: BitDepthRow = .int24
    public var song = SongText()

    public init(document: DocumentModel) { self.document = document }

    public var format: ProjectFormatRow { ProjectFormatRow(sampleRate: rate, depth: depth) }

    /// Reads the project's format and song info.
    public func load() {
        guard let document else { return }
        do {
            loadedFormat = try document.session.projectInfo().format
            loadedSong = SongText(try document.session.songInfo())
        } catch {
            document.alert = AlertItem(error, title: "Project Settings Failed")
        }
        rate = loadedFormat.sampleRate
        depth = loadedFormat.depth
        song = loadedSong
    }

    /// One `setSongInfo` when any text changed, `changeFormat` when the
    /// format changed; nothing when nothing changed.
    public func apply() {
        guard let document else { return }
        if song != loadedSong {
            do {
                document.apply(try document.session.setSongInfo(info: song.row))
                loadedSong = song
            } catch {
                document.alert = AlertItem(error, title: "Project Settings Failed")
            }
        }
        if format != loadedFormat {
            document.changeFormat(format)
            loadedFormat = format
        }
    }
}
