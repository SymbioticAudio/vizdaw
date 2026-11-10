import Foundation
import VizDAWCore

/// The MIDI strip's value arithmetic (plan 5g-1b): volume is CC 7 and pan
/// CC 10, both 0...127; the fader is linear in the CC value (Ruling 5).
public enum MidiStripMath {
    /// What an unset volume shows (Ruling 2).
    public static let defaultVolume: UInt8 = 100
    public static let panCentre: UInt8 = 64

    /// "100" when unset, else the number.
    public static func volumeLabel(_ v: UInt8?) -> String { "\(v ?? defaultVolume)" }

    /// "C", "L 64" ... "R 63"; unset reads as the centre.
    public static func panLabel(_ p: UInt8?) -> String {
        guard let p, p != panCentre else { return "C" }
        return p < panCentre ? "L \(panCentre - p)" : "R \(p - panCentre)"
    }

    /// A whole number 0...127.
    public static func parseVolume(_ s: String) -> UInt8? { number(s.trimmingCharacters(in: .whitespacesAndNewlines)) }

    /// "C", "L n" (1...64), "R n" (1...63) or a plain 0...127.
    public static func parsePan(_ s: String) -> UInt8? {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
        if t == "C" { return panCentre }
        if let side = t.first, side == "L" || side == "R" {
            guard let n = number(t.dropFirst().trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
            if side == "L" { return (1...64).contains(n) ? panCentre - n : nil }
            return (1...63).contains(n) ? panCentre + n : nil
        }
        return number(t)
    }

    /// Fader position 0...1 of a value (an unset one sits at 100).
    public static func position(_ v: UInt8?) -> Double { Double(v ?? defaultVolume) / 127 }

    /// The value at fader position `p`, clamped.
    public static func value(at p: Double) -> UInt8 {
        guard p.isFinite else { return 0 }
        return UInt8((min(max(p, 0), 1) * 127).rounded())
    }

    private static func number(_ t: String) -> UInt8? {
        guard !t.isEmpty, t.utf8.count <= 3, t.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }),
              let n = UInt8(t), n <= 127 else { return nil }
        return n
    }
}

/// One column of the mixer: an audio strip, or an external destination's
/// MIDI strip.
public enum MixerColumn: Identifiable, Equatable {
    case audio(StripRow)
    case midi(DestRow)

    /// "s" + strip ID or "d" + destination ID, so the two never collide.
    public var id: String {
        switch self {
        case .audio(let s): "s\(s.id)"
        case .midi(let d): "d\(d.id)"
        }
    }
}

extension MixerModel {
    /// Least time between two previews of one drag (Ruling 6).
    public static let midiPreviewInterval: Double = 0.020

    /// The columns, left to right (Ruling 7): the strips before the buses,
    /// then every external destination's MIDI strip, then the buses.
    /// Master is not one, as with `strips`.
    public var columns: [MixerColumn] {
        let all = strips
        return all.filter { $0.kind != .bus }.map(MixerColumn.audio)
            + destinations.filter(\.external).map(MixerColumn.midi)
            + all.filter { $0.kind == .bus }.map(MixerColumn.audio)
    }

    /// The mix `dest` shows: a drag's draft, else the committed one.
    public func midiMix(_ dest: UInt64) -> MidiMixRow? {
        midiDraft[dest] ?? destinations.first { $0.id == dest }?.midiMix
    }

    /// A drag moved a control. Sends the CCs without touching the document,
    /// at most once per 20 ms; `nil` leaves that control as it was. `now` is
    /// seconds from any monotonic clock.
    public func midiDrag(_ dest: UInt64, volume: UInt8?, pan: UInt8?, now: Double) {
        guard volume != nil || pan != nil, var mix = midiMix(dest) else { return }
        if midiDraft[dest] == nil { midiDraftBase[dest] = committedMix(dest) }
        if let volume { mix.volume = min(volume, 127) }
        if let pan { mix.pan = min(pan, 127) }
        midiDraft[dest] = mix
        if let last = midiPreviewedAt[dest], now >= last, now - last < Self.midiPreviewInterval { return }
        midiPreviewedAt[dest] = now
        // A preview failing is not worth an alert: the release commits.
        _ = try? document.session.previewMidiMix(dest: dest, volume: volume, pan: pan)
    }

    /// The drag ended: commits what it last moved to, once.
    public func midiRelease(_ dest: UInt64) {
        midiPreviewedAt[dest] = nil
        midiDraftBase[dest] = nil
        guard let mix = midiDraft.removeValue(forKey: dest) else { return }
        // A drag that ended where it began is not an undo step.
        guard mix != committedMix(dest) else { return }
        commitMidiMix(dest, mix)
    }

    /// The drag was abandoned (the gesture cancelled or the view went):
    /// drops its draft without committing. The synth keeps the last
    /// previewed values until the next commit or load.
    public func midiCancel(_ dest: UInt64) {
        midiPreviewedAt[dest] = nil
        midiDraftBase[dest] = nil
        midiDraft[dest] = nil
    }

    func committedMix(_ dest: UInt64) -> MidiMixRow? {
        destinations.first { $0.id == dest }?.midiMix
    }

    /// Drops the drafts of destinations that went or whose committed mix
    /// changed under them (undo, reopen, the control channel).
    func pruneMidiDrafts() {
        for dest in Array(midiDraft.keys) where committedMix(dest) != midiDraftBase[dest]
            || destinations.first(where: { $0.id == dest })?.external != true {
            midiCancel(dest)
        }
    }

    /// Re-reads the destinations so a commit builds on what the document
    /// holds now, not on a refetch a busy plugin editor skipped.
    private func freshMix(_ dest: UInt64) -> MidiMixRow? {
        if let d = try? document.session.destinations() {
            destinations = d
            pruneMidiDrafts()
        }
        return midiMix(dest)
    }

    public func setMidiVolume(_ dest: UInt64, _ v: UInt8) {
        guard var mix = freshMix(dest) else { return }
        mix.volume = min(v, 127)
        commitMidiMix(dest, mix)
    }

    public func setMidiPan(_ dest: UInt64, _ p: UInt8) {
        guard var mix = freshMix(dest) else { return }
        mix.pan = min(p, 127)
        commitMidiMix(dest, mix)
    }

    public func toggleMidiMute(_ dest: UInt64) {
        guard var mix = freshMix(dest) else { return }
        mix.mute.toggle()
        commitMidiMix(dest, mix)
    }

    /// Commits typed text; false (nothing sent) when it does not parse.
    @discardableResult
    public func typeMidiVolume(_ dest: UInt64, _ text: String) -> Bool {
        guard let v = MidiStripMath.parseVolume(text), committedMix(dest) != nil else { return false }
        setMidiVolume(dest, v)
        return true
    }

    @discardableResult
    public func typeMidiPan(_ dest: UInt64, _ text: String) -> Bool {
        guard let p = MidiStripMath.parsePan(text), committedMix(dest) != nil else { return false }
        setMidiPan(dest, p)
        return true
    }

    private func commitMidiMix(_ dest: UInt64, _ mix: MidiMixRow) {
        midiDraft[dest] = nil
        command("Set MIDI Mix Failed") { try $0.setMidiMix(dest: dest, mix: mix) }
    }
}
