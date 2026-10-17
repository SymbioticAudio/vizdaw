import Foundation
import Observation
import VizDAWCore

/// The time step of a nudge (Plan 4b Ruling 16).
public enum NudgeTimeStep: Codable, Hashable, Sendable {
    /// The edit grid's step (also with snap off).
    case grid
    case division(Snap.Division, triplet: Bool)
    case ticks(Int64)
}

/// The pitch step of a nudge.
public enum NudgePitchStep: Codable, Hashable, Sendable {
    case semitones(Int)
    case scaleSteps(Int)
}

public enum NudgeDirection: Sendable {
    case earlier, later, up, down
}

/// The Settings page "Editing" (parent spec §9): both nudge steps.
public struct NudgeSettings: Codable, Equatable, Sendable {
    public var time: NudgeTimeStep = .grid
    public var pitch: NudgePitchStep = .semitones(1)
    /// Scale steps follow the sequence's key signature (major or natural
    /// minor); off: `root` and `scale`.
    public var followKeySignature = true
    public var root: UInt8 = 0
    public var scale: Scale = .major

    public init() {}

    enum CodingKeys: String, CodingKey {
        case time, pitch, followKeySignature, root, scale
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func field<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? c.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let d = NudgeSettings()
        time = field(.time, d.time)
        pitch = field(.pitch, d.pitch)
        followKeySignature = field(.followKeySignature, d.followKeySignature)
        root = field(.root, d.root) % 12
        scale = field(.scale, d.scale)
    }

    /// The time step at `tick` (at least 1).
    public func ticks(at tick: Int64, meter: [MeterEntryRow], snap: Snap) -> Int64 {
        let m = MeterMath.bar(at: tick, meter).meter
        switch time {
        case .grid:
            var s = snap
            s.enabled = true
            return s.step(in: m)
        case let .division(d, triplet):
            var s = Snap()
            s.division = d
            s.triplet = triplet
            return s.step(in: m)
        case .ticks(let n):
            return max(n, 1)
        }
    }

    /// The key scale steps follow at `tick` (Plan 4b Ruling 17).
    public func key(at tick: Int64, meter: [MeterEntryRow]) -> (root: UInt8, mask: UInt16) {
        guard followKeySignature else { return (root % 12, scale.mask) }
        let e = MeterMath.bar(at: tick, meter).meter
        return (ScaleMath.keyRoot(sharps: e.sharps, minor: e.minor), e.minor ? Scale.naturalMinor.mask : Scale.major.mask)
    }
}

/// App-wide editing preferences, kept in UserDefaults (Plan 4b Ruling 16).
@Observable
@MainActor
public final class EditingPreferences {
    public static let key = "VizDAW.nudge"
    @ObservationIgnored private let defaults: UserDefaults

    public var nudge: NudgeSettings {
        didSet {
            if let data = try? JSONEncoder().encode(nudge) { defaults.set(data, forKey: Self.key) }
        }
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        nudge =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(NudgeSettings.self, from: $0) }
            ?? NudgeSettings()
    }
}

extension EditorModel {
    /// Where a nudge reads its grid and key: the selection's range start,
    /// else its earliest selected event among the cached selection lanes,
    /// else tick 0 (Plan 4b Ruling 17); nil without selected lanes.
    public func nudgeAnchor() -> (seq: UInt64, tick: Int64)? {
        guard let first = selection.lanes.first else { return nil }
        if let r = selection.timeRange { return (first.seq, r.start) }
        let ticks = selection.lanes.flatMap { l in
            (laneEvents(LaneKey(l.seq, l.lane))?.events ?? []).filter { isSelected(event: $0.id) }.map(\.tick)
        }
        return (first.seq, ticks.min() ?? 0)
    }

    /// Nudges the selected events one step (parent spec §9); one undo step.
    /// False when nothing is selected or the edit was refused.
    @discardableResult
    public func nudge(_ dir: NudgeDirection, settings: NudgeSettings) -> Bool {
        let events = selection.events
        guard !events.isEmpty else { return false }
        let anchor = nudgeAnchor()
        let meter = anchor.flatMap { info($0.seq)?.meter } ?? []
        let tick = anchor?.tick ?? 0
        switch dir {
        case .earlier, .later:
            let t = settings.ticks(at: tick, meter: meter, snap: layout.snap)
            return edit(.event(edit: .nudge(events: events, ticks: dir == .later ? t : -t, semitones: 0)))
        case .up, .down:
            let sign = dir == .up ? 1 : -1
            switch settings.pitch {
            case .semitones(let n):
                return edit(
                    .event(edit: .nudge(events: events, ticks: 0, semitones: Int8(clamping: sign * max(n, 1)))))
            case .scaleSteps(let n):
                let k = settings.key(at: tick, meter: meter)
                return edit(
                    .event(
                        edit: .transposeInScale(
                            events: events, steps: Int8(clamping: sign * max(n, 1)), root: k.root, mask: k.mask)))
            }
        }
    }
}
