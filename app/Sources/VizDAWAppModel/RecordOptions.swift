import Foundation
import VizDAWCore

/// The record popover and the toolbar's readouts (plan 5d-2 Task 2; spec
/// §6.9): Replace/Overdub and Punch set the transport's switches (as
/// `transport.overdub` and `transport.punch` do), the punch fields edit
/// the 4f punch range, and the readouts say how long the disk lasts, which
/// loop pass is recording and when audio overdub replaces.
extension RecordModel {
    /// Spec §6.4: one lane plays one region at a time.
    public static let overdubNote = "Audio overdub replaces what it overlaps: record into another lane or take to layer."
    /// File › Save while recording (plan 5d-1 Ruling 19).
    public static let savedDuringPass = "Saved. The take is saved when recording stops."
    /// Save of a new, untitled project while recording: it has no package
    /// to save into until the take is stopped.
    public static let saveUntitledWhileRecording =
        "Stop recording to save this new project: its take is being written into it."
    /// Save As while recording: the take is being written inside this
    /// project's package, which the core refuses to move.
    public static let saveAsWhileRecording = "Stop recording to use Save As: the take is being written into this project."
    /// Recording stops below this much free space (spec §6.2: 100 MiB).
    public static let stopBytes: UInt64 = 100 << 20

    /// Replace (overdub off).
    public var replaces: Bool { !(document?.editor.layout.transport.overdub ?? false) }

    public func setReplace(_ on: Bool) { document?.editor.updateLayout { $0.transport.overdub = !on } }

    /// Punch on.
    public var punches: Bool { document?.editor.layout.transport.punch ?? false }

    public func setPunch(_ on: Bool) { document?.editor.updateLayout { $0.transport.punch = on } }

    /// A punch field's text: the typed draft, else the field's value.
    public func punchText(_ f: TransportField) -> String {
        punchDrafts[f] ?? document?.fieldText(f) ?? ""
    }

    /// Keeps a draft for the punch fields only (Task 2 review M5).
    public func setPunchText(_ f: TransportField, _ text: String) {
        guard f == .punchIn || f == .punchOut else { return }
        punchDrafts[f] = text
    }

    /// The popover's punch focus moved (Ruling 12; plan 7d Ruling 7):
    /// moving to the other punch field commits the one it left; leaving
    /// the pair drops its draft (the popover closing has already dealt with
    /// its drafts). Text editing is held while a field has focus.
    public func punchFocusChanged(from old: TransportField?, to new: TransportField?) {
        if let old {
            if new != nil {
                commitPunch(old)
            } else if showsPopover, !keepDraftsOnClose {
                dropPunch(old)
            }
        }
        if new != nil {
            document?.editor.beginTextEditing("recordPopover")
        } else {
            document?.editor.endTextEditing("recordPopover")
        }
    }

    /// Return in a punch field, or moving to the other one: commits its
    /// draft (a refusal says why in the status line and keeps the old
    /// value). A refused draft is dropped: the field shows the old value
    /// again. Returns whether the draft was taken.
    @discardableResult public func commitPunch(_ f: TransportField) -> Bool {
        guard let text = punchDrafts.removeValue(forKey: f) else { return true }
        return document?.commitField(f, text) == true
    }

    /// Drops `f`'s draft: the field shows the value in use again.
    public func dropPunch(_ f: TransportField) {
        punchDrafts[f] = nil
    }

    /// Drops every punch draft (the popover closing, plan 7d Ruling 7):
    /// the fields show the range a pass will use.
    public func dropPunchDrafts() {
        punchDrafts.removeAll()
    }

    /// A punch field shows typed text that is not the value in use: Record
    /// waits for Return or Esc (controller ruling I5).
    public var hasOpenPunchDraft: Bool {
        punchDrafts.contains { f, text in text != (document?.fieldText(f) ?? "") }
    }

    /// A press on the bar's Record button begins with a draft open (its
    /// mouse-down monitor calls this before the popover's outside-click
    /// handling sees the press): the popover's close keeps the drafts, so
    /// `pressRecord()` can refuse and show them again.
    public func recordPressBegins() {
        guard showsPopover, hasOpenPunchDraft else { return }
        keepDraftsOnClose = true
    }

    /// The count-in is playing (the timeline waits).
    public var countingIn: Bool { document?.transport.countingIn ?? false }

    /// The toolbar's Record button: like the R key, and a refusal shows in
    /// the status line. Refused for an open punch draft, it opens the
    /// popover again showing the draft (Ruling 7's bar Record rule).
    public func pressRecord() {
        // A press that did not close the popover leaves no flag behind.
        keepDraftsOnClose = false
        let outcome = toggle()
        switch outcome {
        case .done: break
        case .refused(let why), .busy(let why), .unavailable(let why): document?.editor.status = why
        }
        if outcome == .refused(Self.openDraftMessage), !showsPopover { showsPopover = true }
    }

    /// The channels the shown sequence's armed audio lanes record.
    public var armedChannels: Int {
        (document?.lanes ?? []).filter(\.arm).reduce(0) { n, lane in
            if case .audio(_, let stereo, _) = lane.input { return n + (stereo ? 2 : 1) }
            return n
        }
    }

    /// Free bytes on the volume of `path` (the project; nil: untitled,
    /// whose recordings go to the temporary folder's volume).
    nonisolated public static func volumeFreeSpace(_ path: String?) -> UInt64? {
        let url = URL(fileURLWithPath: path ?? NSTemporaryDirectory())
        let v = try? url.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        return v?.volumeAvailableCapacityForImportantUsage.flatMap { UInt64(exactly: $0) }
    }

    /// Seconds of recording the disk holds before recording stops (spec
    /// §6.9: from the free space and the armed channels); nil with no
    /// armed audio lane.
    public var remainingSeconds: Double? {
        guard let m = document, let format = m.projectInfo?.format else { return nil }
        let channels = armedChannels
        guard channels > 0 else { return nil }
        let bytes: Double =
            switch format.depth {
            case .int16: 2
            case .int24: 3
            case .float32: 4
            }
        let perSecond = Double(format.sampleRate) * Double(channels) * bytes
        let free = isRecording ? state?.freeBytes : Self.freeSpace((try? m.session.path()) ?? nil)
        guard let free, perSecond > 0 else { return nil }
        return Double(free > Self.stopBytes ? free - Self.stopBytes : 0) / perSecond
    }

    /// "1 h 5 min left"; nil with no armed audio lane.
    public var remainingText: String? { remainingSeconds.map(Self.remaining) }

    static func remaining(_ seconds: Double) -> String {
        guard seconds >= 60 else { return "Less than a minute left" }
        let minutes = Int(seconds / 60)
        guard minutes >= 60 else { return "\(minutes) min left" }
        let (h, r) = (minutes / 60, minutes % 60)
        return r == 0 ? "\(h) h left" : "\(h) h \(r) min left"
    }

    /// The loop record counters (spec §6.9); nil unless loop recording.
    public var passText: String? {
        guard isLoopRecording, let s = state else { return nil }
        var t = "Pass \(s.pass)"
        if s.kept > 0 { t += " · \(s.kept) kept" }
        if s.discarded > 0 { t += " · \(s.discarded) discarded" }
        return t
    }

    /// Spec §6.4's note, shown in the popover with overdub on and an audio
    /// lane armed.
    public var overdubNote: String? {
        guard !replaces else { return nil }
        let audio = (document?.lanes ?? []).contains { lane in
            if lane.arm, case .audio = lane.input { return true }
            return false
        }
        return audio ? Self.overdubNote : nil
    }
}
