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

    /// The fader's 0...127 scale (Ruling 5: linear in the CC value).
    public static let faderMarks: [ScaleMark] = [127, 100, 64, 32, 0].map {
        ScaleMark(position: Double($0) / 127, label: "\($0)")
    }

    /// The pan knob's position 0...1 (an unset pan sits at the centre).
    public static func panPosition(_ p: UInt8?) -> Double { Double(p ?? panCentre) / 127 }

    /// The strip's kind line: "MIDI · Ch 1" (the channel is 0...15).
    public static func kindLabel(channel: UInt8?) -> String {
        channel.map { "MIDI \u{00B7} Ch \(Int($0) + 1)" } ?? "MIDI"
    }

    /// The left pane's header for a MIDI track without a destination
    /// (§6.8a), as the track header's menu writes it.
    public static let noDestination = "No Destination"
    /// How much an unset value (or a control that is off) is dimmed: the
    /// opacity the strips' off controls use.
    public static let dimmed = 0.45
    /// The note-activity meter's width beside the fader.
    public static let meterWidth: CGFloat = 6
    /// The status line after refused typed text.
    public static let volumeHint = "Type a volume from 0 to 127."
    public static let panHint = "Type a pan like L 24, C or R 10, or 0 to 127."

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

    /// The columns, left to right (Ruling 7, final review m1): the strips
    /// in the document's order, with every external destination's MIDI
    /// strip inserted before the first bus (at the end when there is
    /// none), so an interleaved order such as [Audio 1, Bus 1, Audio 2]
    /// keeps its places. Master is not one, as with `strips`.
    public var columns: [MixerColumn] {
        var out = strips.map(MixerColumn.audio)
        let at = strips.firstIndex { $0.kind == .bus } ?? out.count
        out.insert(contentsOf: destinations.filter(\.external).map(MixerColumn.midi), at: at)
        return out
    }

    /// Whether `dest`'s strip shows as selected in the mixer (Task 6
    /// review M1): a focused track (`focusLanes`: the selection, else the
    /// armed tracks) plays it.
    public func isMidiSelected(_ dest: UInt64) -> Bool {
        document.focusLanes().contains { $0.midiDest == dest }
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
    /// drops its draft without committing, and sends the committed volume
    /// and pan back once when they are set (final review m3), so the synth
    /// matches the strip again; unset ones send nothing.
    public func midiCancel(_ dest: UInt64) {
        midiPreviewedAt[dest] = nil
        midiDraftBase[dest] = nil
        guard midiDraft.removeValue(forKey: dest) != nil, let c = committedMix(dest),
            c.volume != nil || c.pan != nil
        else { return }
        _ = try? document.session.previewMidiMix(dest: dest, volume: c.volume, pan: c.pan)
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

    /// The key a MIDI strip's MIDI FX settings popover shows under
    /// (`InputFxModel.SettingsSlot.strip`): the destination, else the track,
    /// with the top bit set so it never meets an audio strip's ID.
    public static func midiFxKey(dest: UInt64?, lane: UInt64) -> UInt64 {
        dest.map { $0 | (1 << 63) } ?? (lane | (3 << 62))
    }

    /// The strip's R: toggles `lane`'s arm (one undo step), as the track
    /// header does.
    public func toggleMidiArm(_ lane: LaneRow) {
        guard let seq = document.selectedSeq else { return }
        _ = document.editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane.id, mute: nil, solo: nil, arm: !lane.arm)))
    }

    /// The left pane's destination menu (Ruling 8): sets `lane`'s
    /// destination, as the track header's menu does; nil is none.
    public func setMidiDest(_ lane: LaneRow, _ dest: UInt64?) {
        guard let seq = document.selectedSeq, lane.midiDest != dest else { return }
        _ = document.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: dest)))
    }

    private func commitMidiMix(_ dest: UInt64, _ mix: MidiMixRow) {
        midiDraft[dest] = nil
        command("Set MIDI Mix Failed") { try $0.setMidiMix(dest: dest, mix: mix) }
    }
}
