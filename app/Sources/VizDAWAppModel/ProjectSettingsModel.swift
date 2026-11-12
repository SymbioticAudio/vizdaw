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
    /// The sheet is up (plan 6a Task 10; never under the control channel).
    public var shown = false
    /// What a format change would convert, read by `load()`.
    public private(set) var mediaNote = ""

    public init(document: DocumentModel) { self.document = document }

    public var format: ProjectFormatRow { ProjectFormatRow(sampleRate: rate, depth: depth) }

    /// File › Project Settings…: loads, then shows the sheet.
    public func show() {
        load()
        shown = true
    }

    /// The sheet's line about converting the project's audio files.
    static func note(_ media: [MediaRow]) -> String {
        guard !media.isEmpty else { return "The project has no audio files yet." }
        let seconds = media.reduce(0.0) { $0 + Double($1.frames) / Double(max($1.sampleRate, 1)) }
        let length = seconds >= 60 ? String(format: "%.0f min", seconds / 60) : String(format: "%.0f s", seconds)
        let files = media.count == 1 ? "1 audio file" : "\(media.count) audio files"
        return "Changing the format converts \(files) (\(length) of audio) into new files (one undo step)."
    }

    /// Reads the project's format and song info.
    public func load() {
        guard let document else { return }
        mediaNote = Self.note((try? document.session.media()) ?? [])
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
            if document.changeFormat(format) { loadedFormat = format }
        }
    }
}
