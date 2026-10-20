import Foundation
import Observation
import VizDAWCore

/// One media-pool row as the window shows it (spec §10.3).
public struct MediaPoolRow: Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let name: String
    public let duration: String
    public let channels: String
    public let origin: String
    public let uses: UInt32
    public let used: String
    /// Why it is offline; empty when online.
    public let offline: String
}

/// A document's media pool (spec §10.3; plan 5b Task 8): the rows, the
/// pool window's selection, the relink sheet and the pool's actions. The
/// rows are refetched when `DocumentModel.mediaVersion` moves.
@Observable
@MainActor
public final class MediaModel {
    /// Weak (preflight I4): a pool window can outlive its document; without
    /// one, reads answer the cached rows and actions do nothing.
    @ObservationIgnored public weak var document: DocumentModel?
    /// The relinkable items the sheet was already offered for (preflight
    /// I3), so Leave Offline is not undone by the same items.
    @ObservationIgnored private var offered: Set<UInt64> = []
    @ObservationIgnored private var cache: (version: Int, rows: [MediaRow])?
    /// The pool window's selected items.
    public var selection: Set<UInt64> = []
    /// The relink sheet is up (Ruling 14).
    public var relinkShown = false

    init(document: DocumentModel) { self.document = document }

    /// The pool. Refetched when the media version moved; a failed read
    /// keeps the last rows. Safe to read while drawing (the cache is not
    /// observed state).
    public var rows: [MediaRow] {
        guard let document else { return cache?.rows ?? [] }
        let v = document.mediaVersion
        if let c = cache, c.version == v { return c.rows }
        let r = (try? document.session.media()) ?? cache?.rows ?? []
        cache = (v, r)
        return r
    }

    public func row(_ id: UInt64?) -> MediaRow? {
        guard let id else { return nil }
        return rows.first { $0.id == id }
    }

    public func offline(_ id: UInt64?) -> OfflineReasonRow? { row(id)?.offline }

    /// The reasons the relink sheet lists (another format or an unreadable
    /// file get the Convert banner instead).
    public static let relinkReasons: Set<OfflineReasonRow> = [.missing, .length, .changed]

    public var relinkable: [MediaRow] {
        rows.filter { $0.offline.map(Self.relinkReasons.contains) ?? false }
    }

    public var poolRows: [MediaPoolRow] { rows.map(Self.poolRow) }

    public static func poolRow(_ r: MediaRow) -> MediaPoolRow {
        MediaPoolRow(
            id: r.id, name: r.name, duration: duration(frames: r.frames, rate: r.sampleRate),
            channels: r.channels == 1 ? "Mono" : r.channels == 2 ? "Stereo" : "\(r.channels) channels",
            origin: originText(r.origin), uses: r.uses, used: String(format: "%.0f %%", r.usedPercent),
            offline: r.offline.map(reasonText) ?? "")
    }

    /// `m:ss.t`, or `h:mm:ss.t` from an hour on.
    public static func duration(frames: UInt64, rate: UInt32) -> String {
        let tenths = Int((Double(frames) / Double(max(rate, 1)) * 10).rounded())
        let (h, rest) = (tenths / 36_000, tenths % 36_000)
        let (m, t) = (rest / 600, rest % 600)
        let s = String(format: "%02d.%d", t / 10, t % 10)
        return h > 0 ? "\(h):" + String(format: "%02d", m) + ":" + s : "\(m):" + s
    }

    public static func originText(_ o: MediaOriginRow) -> String {
        switch o {
        case .imported: "Imported"
        case .recorded: "Recorded"
        case .bounced: "Bounced"
        case .externalEdit: "External Edit"
        }
    }

    public static func reasonText(_ r: OfflineReasonRow) -> String {
        switch r {
        case .missing: "Missing"
        case .unreadable: "Not a VizDAW file"
        case .format: "Another format"
        case .length: "Changed length"
        case .changed: "Changed"
        }
    }

    // MARK: Actions

    /// A pool edit (one undo step); refused while a drag is open; a
    /// failure is the document's alert.
    @discardableResult
    private func act(_ title: String, _ body: (any SessionProtocol) throws -> DocSnapshot) -> Bool {
        guard let document else { return false }
        guard document.editor.gesture == nil else {
            document.editor.status = EditorModel.gestureOpenMessage
            return false
        }
        do {
            document.apply(try body(document.session))
            document.editor.refreshSelection()
            document.mediaVersion += 1
            return true
        } catch {
            document.alert = AlertItem(error, title: title)
            return false
        }
    }

    /// Rename (trimmed; a blank or unchanged name does nothing).
    @discardableResult
    public func rename(_ id: UInt64, to name: String) -> Bool {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty, n != row(id)?.name else { return false }
        return act("Rename Failed") { try $0.renameMedia(media: id, name: n) }
    }

    /// Compact: every unused item goes (one undo step).
    @discardableResult
    public func compact() -> Bool { act("Compact Failed") { try $0.compactMedia() } }

    /// Deletes the item and its regions (ask with `deleteQuestion` first).
    @discardableResult
    public func remove(_ id: UInt64) -> Bool {
        let ok = act("Delete Failed") { try $0.removeMedia(media: id) }
        if ok { selection.remove(id) }
        return ok
    }

    public func users(_ id: UInt64) -> [MediaUseRow] {
        guard let document else { return [] }
        return (try? document.session.mediaUsers(media: id)) ?? []
    }

    /// What Delete asks first (parent spec §5): nil when no region uses
    /// the item.
    public func deleteQuestion(_ id: UInt64) -> String? {
        let u = users(id)
        guard let document, !u.isEmpty, let r = row(id) else { return nil }
        let places = u.prefix(8).map { use in
            let meter = document.editor.info(use.seq)?.meter ?? []
            return "\(use.sequence) › \(use.laneName), bar \(MeterMath.bar(at: use.tick, meter).number)"
        }
        let more = u.count > 8 ? "\n… and \(u.count - 8) more" : ""
        let n = u.count
        return "“\(r.name)” is used by \(n) region\(n == 1 ? "" : "s"):\n" + places.joined(separator: "\n") + more
            + "\nDelete the item and these regions?"
    }

    /// Selects the regions using `id` in the first sequence that has one,
    /// and shows that sequence (Ruling 13).
    public func selectUsers(_ id: UInt64) {
        let u = users(id)
        guard let document else { return }
        guard let first = u.first else {
            document.editor.status = "No region uses this item."
            return
        }
        document.choose(first.seq)
        let mine = u.filter { $0.seq == first.seq }
        var lanes: [LaneRef] = []
        for use in mine where !lanes.contains(LaneRef(seq: use.seq, lane: use.lane)) {
            lanes.append(LaneRef(seq: use.seq, lane: use.lane))
        }
        document.editor.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: mine.map(\.event)))
    }

    public func selectUnused() { selection = Set(rows.filter { $0.uses == 0 }.map(\.id)) }

    /// Locate…: relinks `id` to the file at `path` (a job).
    public func relink(_ id: UInt64, path: String) {
        document?.startJob(.relink, "Relink Failed") { try $0.relinkMedia(media: id, path: path) }
    }

    /// Search folder…: every missing or changed item (a job).
    public func searchFolder(_ path: String) {
        document?.startJob(.relink, "Relink Failed") { try $0.searchMedia(folder: path) }
    }

    /// The pool's Import…: into the pool only, no regions.
    public func importIntoPool(_ paths: [String]) { document?.importAudio(paths: paths, target: nil) }

    /// The item's file (Reveal in Finder).
    public func file(_ id: UInt64) -> String? { try? document?.session.mediaFile(media: id) }

    /// Shows the relink sheet when items are missing or changed that it
    /// was not yet offered for: on open (Ruling 14) and again when a later
    /// idle or recheck finds more (`Changed` is known only after the worker
    /// re-hashed the file; preflight I3). Items that went back online are
    /// forgotten, so a new loss is offered again.
    public func offerRelinkIfNeeded() {
        let ids = Set(relinkable.map(\.id))
        let fresh = ids.subtracting(offered)
        offered = ids
        if !fresh.isEmpty { relinkShown = true }
    }

    /// "Leave Offline".
    public func leaveOffline() { relinkShown = false }

    /// Region `event`'s waveform over ticks `[fromTick, toTick)` in
    /// `columns` columns (at most 8192); nil when it cannot be read
    /// (nothing is drawn).
    public func waveform(event: UInt64, fromTick: Int64, toTick: Int64, columns: Int) -> OverviewRows? {
        guard columns > 0, fromTick < toTick else { return nil }
        guard let document else { return nil }
        return try? document.session.regionWaveform(
            event: event, fromTick: fromTick, toTick: toTick, columns: UInt32(min(columns, 8192)))
    }
}
