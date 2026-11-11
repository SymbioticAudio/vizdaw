import Foundation
import SymbioticTokens
import VizDAWCore

/// The left pane's "Channel Strip" rule (plan 7a Ruling 5): which strip the
/// selected lane plays through, the inspector when it has none, or nothing.
public enum StripFocus {
    public enum Result: Equatable {
        case strip(UInt64)
        /// A MIDI track's strip: `dest` is its external destination (nil
        /// when it has none), `lane` the focused track.
        case midi(dest: UInt64?, lane: UInt64)
        case inspector
        case nothing
    }

    public static let nothingMessage = "Select a track to see its channel strip."

    /// The strip `lane` plays through: its audio path's, its plugin
    /// instrument's, or the built-in synth's; nil for an external
    /// destination or none.
    public static func strip(of lane: LaneRow, destinations: [DestRow], strips: [StripRow]) -> UInt64? {
        if let p = lane.audioOut {
            return strips.first { $0.kind == .audioPath(path: p) }?.id
        }
        guard let d = lane.midiDest, let dest = destinations.first(where: { $0.id == d }) else { return nil }
        if dest.plugin != nil {
            return strips.first { $0.kind == .instrument(dest: d) }?.id
        }
        if dest.external { return nil }
        return strips.first { $0.kind == .builtinSynth }?.id
    }

    /// The selection's lanes, else the first armed lane, decide.
    public static func strip(
        selected: [LaneRow], armed: [LaneRow], destinations: [DestRow], strips: [StripRow]
    ) -> Result {
        guard let first = selected.first ?? armed.first else { return .nothing }
        if let id = strip(of: first, destinations: destinations, strips: strips) { return .strip(id) }
        guard first.kind == .midi else { return .inspector }
        guard let d = first.midiDest else { return .midi(dest: nil, lane: first.id) }
        let external = destinations.first { $0.id == d }?.external ?? false
        return external ? .midi(dest: d, lane: first.id) : .inspector
    }
}

extension DocumentModel {
    /// The selected lanes of the shown sequence, then its armed lanes: the
    /// first is the one the side panes follow.
    public func focusLanes() -> [LaneRow] {
        let selected: [LaneRow] = editor.selection.lanes.compactMap { ref in
            guard ref.seq == selectedSeq else { return nil }
            return lanes.first { $0.id == ref.lane }
        }
        return selected.isEmpty ? lanes.filter(\.arm) : selected
    }

    /// The single selected event of the shown sequence when it is an
    /// audio region (the followed lane is looked in first), with its lane
    /// and media row; nil for none, several, a note or a reference (plan
    /// 7c Ruling 2). The inspector and the audio editor share it.
    public var editedRegion: EditedRegion? {
        let events = editor.selection.events
        guard events.count == 1, let id = events.first, let seq = selectedSeq else { return nil }
        let first = focusLanes().first
        let order = (first.map { [$0] } ?? []) + lanes.filter { $0.id != first?.id }
        for lane in order {
            let key = LaneKey(seq, lane.id)
            if let e = editor.laneEvents(key)?.events.first(where: { $0.id == id }) {
                guard e.kind == .audioRegion else { return nil }
                return EditedRegion(row: e, lane: key, media: media.row(e.media))
            }
        }
        return nil
    }

    /// The inspector's region: `editedRegion`'s event.
    public var inspectorRegion: EventRow? { editedRegion?.row }

    /// The left pane's inspector rows (Ruling 6): the followed lane, the
    /// single selected audio region, with fades in ms at the file's rate
    /// (the project's when the media row is missing or has no rate).
    public func inspectorRows(destinations: [DestRow], strips: [StripRow]) -> [InspectorRow] {
        let region = inspectorRegion
        return Inspector.rows(
            lane: focusLanes().first, region: region, regionName: media.row(region?.media)?.name,
            destinations: destinations, strips: strips,
            sampleRate: Double(fileRate(of: editedRegion?.media) ?? projectInfo?.format.sampleRate ?? 0))
    }

    /// A media row's own rate; nil for no row or a 0 rate.
    private func fileRate(of media: MediaRow?) -> UInt32? {
        guard let rate = media?.sampleRate, rate > 0 else { return nil }
        return rate
    }

    /// `lane`'s colour in Arrange, through the document's mixer
    /// (`StripPalette.lane`), made on demand; nil when it cannot be made.
    public func laneColour(_ lane: LaneRow, _ t: Theme) -> ThemeColor? {
        guard let m = mixer else { return nil }
        return StripPalette.lane(lane, destinations: m.destinations, strips: m.snapshot.strips, t)
    }

    /// What the left pane's strip view shows now.
    public func leftStrip(strips: [StripRow], destinations: [DestRow]) -> StripFocus.Result {
        StripFocus.strip(selected: focusLanes(), armed: [], destinations: destinations, strips: strips)
    }
}

/// The one audio region the inspector and the audio editor show.
public struct EditedRegion: Equatable, Sendable {
    public let row: EventRow
    public let lane: LaneKey
    public let media: MediaRow?

    public init(row: EventRow, lane: LaneKey, media: MediaRow?) {
        self.row = row
        self.lane = lane
        self.media = media
    }
}

public struct InspectorRow: Equatable, Sendable {
    public let label: String
    public let value: String

    public init(label: String, value: String) {
        self.label = label
        self.value = value
    }
}

/// The left pane's read-only inspector (plan 7a Ruling 6).
public enum Inspector {
    public static let emptyMessage = "Select a track or a region."

    /// `region` is an `EventRow` (an audio region event); `sampleRate` turns
    /// its fades' frames into ms (without it they show as frames).
    public static func rows(
        lane: LaneRow?, region: EventRow?, regionName: String?, destinations: [DestRow],
        strips: [StripRow], sampleRate: Double = 0
    ) -> [InspectorRow] {
        var out: [InspectorRow] = []
        if let lane {
            var plays = "Nothing"
            if let p = lane.audioOut {
                if let s = strips.first(where: { $0.kind == .audioPath(path: p) }) { plays = "Audio: \(s.name)" }
            } else if let d = lane.midiDest, let dest = destinations.first(where: { $0.id == d }) {
                plays = dest.name
            }
            out += [
                InspectorRow(label: "Name", value: lane.name),
                InspectorRow(label: "Plays", value: plays),
                InspectorRow(label: "Input", value: LaneInputs.title(lane: lane)),
                InspectorRow(label: "Monitor", value: LaneInputs.label(lane.monitor)),
                InspectorRow(label: "Takes", value: "\(lane.takeCount)"),
                InspectorRow(label: "Armed", value: lane.arm ? "Yes" : "No"),
            ]
        }
        if let region {
            out += [
                InspectorRow(label: "Region", value: regionName ?? "Region"),
                InspectorRow(label: "Gain", value: String(format: "%.1f dB", region.gainDb)),
                InspectorRow(label: "Fade In", value: fadeText(region.fadeIn.length, sampleRate)),
                InspectorRow(label: "Fade Out", value: fadeText(region.fadeOut.length, sampleRate)),
            ]
        }
        return out
    }

    private static func fadeText(_ frames: Int64, _ rate: Double) -> String {
        guard rate > 0 else { return "\(frames) frames" }
        return String(format: "%.0f ms", Double(frames) / rate * 1000)
    }
}

public struct MarkerListRow: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let tick: Int64
    public let position: String

    public init(id: String, name: String, tick: Int64, position: String) {
        self.id = id
        self.name = name
        self.tick = tick
        self.position = position
    }
}

/// The right pane's marker list: the sequence's markers and the set locators.
public enum MarkerList {
    public static func rows(info: SequenceInfo?, marks: TransportMarks) -> [MarkerListRow] {
        let meter = info?.meter ?? []
        func position(_ t: Int64) -> String {
            let b = Bbu.of(t, meter: meter)
            return "\(b.bar).\(b.beat).\(b.unit)"
        }
        var items: [(row: MarkerListRow, rank: Int)] = []
        for (i, m) in (info?.markers ?? []).enumerated() {
            items.append((MarkerListRow(id: "m\(i)", name: m.name, tick: m.tick, position: position(m.tick)), 0))
        }
        for n in marks.locators.indices {
            guard let t = marks.locator(n) else { continue }
            items.append((MarkerListRow(id: "l\(n)", name: "Locator \(n)", tick: t, position: position(t)), 1))
        }
        return items.enumerated().sorted {
            ($0.element.row.tick, $0.element.rank, $0.offset) < ($1.element.row.tick, $1.element.rank, $1.offset)
        }.map(\.element.row)
    }

    @MainActor public static func go(_ row: MarkerListRow, _ m: DocumentModel) {
        do { try m.moveTo(row.tick) } catch { m.editor.status = "Go to Marker Failed: \(error)" }
    }
}

/// The right pane's actions.
@MainActor public enum PaneContent {
    public static let noStripMessage = "Select a track or one mixer strip first."

    /// Puts `p` on the end of the insert chain of the strip the view points
    /// at (Ruling 7); returns the refusal text, or nil when it went in.
    public static func insertPlugin(_ p: PluginRefRow, document: DocumentModel, mixer: MixerModel) -> String? {
        let masterID = mixer.master?.id
        var selected: UInt64?
        if document.selectedStrips.count == 1, let id = document.selectedStrips.first, id != masterID {
            selected = id
        }
        var fromLane: UInt64?
        if case .strip(let id) = document.leftStrip(strips: mixer.snapshot.strips, destinations: mixer.destinations) {
            fromLane = id
        }
        let strip =
            document.editor.layout.views.main == .mix ? (selected ?? fromLane) : (fromLane ?? selected)
        guard let strip else { return noStripMessage }
        mixer.browser = .insert(strip: strip)
        mixer.pick(p)
        return nil
    }
}
