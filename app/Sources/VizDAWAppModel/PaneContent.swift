import Foundation
import VizDAWCore

/// The left pane's "Channel Strip" rule (plan 7a Ruling 5): which strip the
/// selected lane plays through, the inspector when it has none, or nothing.
public enum StripFocus {
    public enum Result: Equatable {
        case strip(UInt64)
        case inspector
        case nothing
    }

    public static let nothingMessage = "Select a lane to see its channel strip."

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
        guard let id = strip(of: first, destinations: destinations, strips: strips) else { return .inspector }
        return .strip(id)
    }
}

extension DocumentModel {
    /// What the left pane's strip view shows now.
    public func leftStrip(strips: [StripRow], destinations: [DestRow]) -> StripFocus.Result {
        let selected: [LaneRow] = editor.selection.lanes.compactMap { ref in
            guard ref.seq == selectedSeq else { return nil }
            return lanes.first { $0.id == ref.lane }
        }
        return StripFocus.strip(
            selected: selected, armed: lanes.filter(\.arm), destinations: destinations, strips: strips)
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
    public static let emptyMessage = "Select a lane or a region."

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
                InspectorRow(label: "Input", value: LaneInputs.title(lane.input)),
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
