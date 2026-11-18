import Foundation
import VizDAWCore

/// Settings › Audio's and › Recording's readouts and choices (plan 5d-2
/// Task 7; spec §6.6, §6.9).
extension AudioPreferences {
    /// The input devices the picker offers, with their UIDs (tests
    /// replace it).
    public static var inputDeviceList: () -> [InputDeviceRow] = { audioInputDeviceList() }

    /// The RAM cache sizes offered, MiB (64–8192; spec §5.5: 512).
    public static let cacheChoices: [UInt32] = [64, 128, 256, 512, 1024, 2048, 4096, 8192]

    /// `list`, with `current` added in order when it is not on it.
    /// (Task 7 review Minor 1: a picker never shows an empty selection.)
    public static func choices(_ list: [UInt32], keeping current: UInt32) -> [UInt32] {
        list.contains(current) ? list : (list + [current]).sorted()
    }

    /// The input device picker: "Same as output" (nil), every device, and
    /// the chosen one when it is not connected. Two devices with one name
    /// are "<name>" and "<name> (2)", in list order (release fixes Task 2).
    public func inputDeviceChoices() -> [(title: String, uid: String?, name: String?)] {
        var out: [(title: String, uid: String?, name: String?)] = [("Same as output", nil, nil)]
        var seen: [String: Int] = [:]
        for d in Self.inputDeviceList() {
            let n = (seen[d.name] ?? 0) + 1
            seen[d.name] = n
            out.append((n == 1 ? d.name : "\(d.name) (\(n))", d.uid.isEmpty ? nil : d.uid, d.name))
        }
        let chosen = inputDeviceTag()
        if let chosen, !out.contains(where: { Self.tag(of: $0) == chosen }) {
            let s = settings
            out.append(("\(s.inputDevice ?? chosen) (not connected)", s.inputDeviceUID, s.inputDevice))
        }
        return out
    }

    /// A choice's picker tag: its UID, else its name (nil: "Same as
    /// output").
    public static func tag(of c: (title: String, uid: String?, name: String?)) -> String? { c.uid ?? c.name }

    /// The chosen device's picker tag: its UID; for a name alone (older
    /// settings), the UID of the only listed device with that name, else
    /// the name.
    public func inputDeviceTag() -> String? {
        if let uid = settings.inputDeviceUID { return uid }
        guard let name = settings.inputDevice else { return nil }
        let named = Self.inputDeviceList().filter { $0.name == name && !$0.uid.isEmpty }
        return named.count == 1 ? named.first?.uid : name
    }

    /// The picker's choice tagged `tag`.
    public func selectInputDevice(tag: String?) {
        guard let c = inputDeviceChoices().first(where: { Self.tag(of: $0) == tag }) else { return }
        setInputDevice(uid: c.uid, name: c.name)
    }

    /// The first open session whose device has opened an input: the one
    /// whose latencies and round trip Settings shows.
    public static func inputHolder(_ sessions: [any SessionProtocol]) -> (any SessionProtocol)? {
        sessions.first { ((try? $0.inputInfo())?.device ?? "") != "" }
    }

    /// The device's input as last read (Settings opens, the buffer size
    /// changes): its latencies and the round trip.
    public func readDevice(_ session: (any SessionProtocol)?) {
        let info = try? session?.inputInfo()
        if info != device { device = info }
    }

    /// The round-trip readout (spec §6.6): the device's measured round
    /// trip once it has opened, else the buffer's share of it.
    public static func roundTripText(_ info: InputInfoRow?, bufferFrames: UInt32) -> String {
        guard let info, !info.device.isEmpty, info.sampleRate > 0, info.roundTripFrames > 0 else {
            let ms = 2 * Double(bufferFrames) / 48_000 * 1_000
            return String(format: "About %.1f ms at 48 kHz, plus the device's own latency once it opens", ms)
        }
        let ms = Double(info.roundTripFrames) / info.sampleRate * 1_000
        var text = String(
            format: "Round trip %.1f ms (input %u + output %u + 2 × %u frames)", ms, info.inputLatencyFrames,
            info.outputLatencyFrames, info.bufferFrames)
        if info.bufferFrames != bufferFrames {
            text += "; \(info.device) runs at \(info.bufferFrames) frames"
        }
        return text
    }

    /// The latency trim's readout: "+12 frames (0.25 ms)".
    public static func trimText(_ frames: Int32, sampleRate: Double) -> String {
        let sign = frames > 0 ? "+" : ""
        let rate = sampleRate > 0 ? sampleRate : 48_000
        return String(format: "\(sign)\(frames) frames (%.2f ms)", Double(frames) / rate * 1_000)
    }

    /// Spec §6.2's thresholds, shown only.
    public static let lowDiskText = "Recording warns below 1 GB of free disk space and stops below 100 MB."
}
