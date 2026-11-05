import Foundation
import Observation
import VizDAWCore

/// Where the record popover shows (plan 7a final review I2): the document
/// window's transport bar or the mixer window's.
public enum RecordPopoverHost: Equatable, Sendable { case document, mixerWindow }

/// Recording in a document (plan 5c Task 13; sub-project 5 spec §6.4,
/// §6.5): what Record asks the core for, what the core reports while it
/// records, and the crash-recovery offer. Replace, punch and loop come
/// from the transport (overdub, the punch fields, Loop); count-in,
/// pre-roll and passes-as-takes are kept here for the record popover (and
/// `record.set`), starting from Settings › Recording's defaults.
@Observable
@MainActor
public final class RecordModel {
    @ObservationIgnored weak var document: DocumentModel?
    /// What a new document's options start as (Settings › Recording; plan
    /// 5d-2 Ruling 3).
    public static var defaults = RecordDefaults()
    /// Bars of count-in (0–4).
    public var countInBars: UInt8 = 0 {
        didSet { if countInBars > 0 { lastCountIn = countInBars } }
    }
    /// The last non-zero count-in, for the keyboard's Count-in toggle
    /// (plan 5f-4 Ruling 16).
    public private(set) var lastCountIn: UInt8 = 1
    /// Bars of pre-roll (0–8).
    public var preRollBars: UInt8 = 0
    /// Kept loop passes become takes.
    public var passesAsTakes = true
    /// The toolbar's record popover is open; closing it drops the punch
    /// drafts (plan 7d Ruling 7), unless the press that closed it began
    /// on the bar's Record button (`recordPressBegins()`).
    public var showsPopover = false {
        didSet {
            if !showsPopover {
                if keepDraftsOnClose {
                    keepDraftsOnClose = false
                } else {
                    dropPunchDrafts()
                }
                document?.editor.endTextEditing("recordPopover")
            }
        }
    }
    /// The next close of the popover keeps the punch drafts: a press on the
    /// bar's Record button began with a draft open (Ruling 7's bar Record
    /// rule), so Record can refuse and show it again.
    public internal(set) var keepDraftsOnClose = false
    /// The window whose chevron opened the popover (plan 7a final review
    /// I2): the mixer window has a transport bar too, and the popover shows
    /// under one chevron only.
    public private(set) var popoverHost: RecordPopoverHost = .document

    /// The chevron in `host`: closes the popover open there, else opens it
    /// there (closing it elsewhere first, which drops the drafts).
    public func togglePopover(in host: RecordPopoverHost) {
        if showsPopover(in: host) {
            showsPopover = false
            return
        }
        if showsPopover { showsPopover = false }
        popoverHost = host
        showsPopover = true
    }

    /// The popover is open in `host`.
    public func showsPopover(in host: RecordPopoverHost) -> Bool { showsPopover && popoverHost == host }
    /// Text typed in the popover's punch fields, until Return, focus
    /// moving within or leaving the pair, or the popover closing.
    var punchDrafts: [TransportField: String] = [:]
    /// Free bytes on a project's volume (tests replace it; plan 5d-2 Task
    /// 2).
    public static var freeSpace: (String?) -> UInt64? = RecordModel.volumeFreeSpace
    /// What the core last reported.
    public private(set) var state: RecordStateRow?
    /// Recordings found in the project and not yet answered: crash
    /// leftovers and takes nothing names (never offered under the control
    /// channel).
    public private(set) var recoveryOffer: [RecoveredFileRow] = []

    /// Why Record is refused while step entry is on (spec §6.4).
    public static let stepEntryOn = "Step entry is on: turn it off to record."
    /// Why Record is refused while a punch draft is open (plan 7d
    /// controller ruling I5).
    public static let openDraftMessage = "Press Return to set the punch, or Esc to discard it."

    init(document: DocumentModel) {
        self.document = document
        let d = Self.defaults
        countInBars = d.countInBars
        lastCountIn = max(d.countInBars, 1)
        preRollBars = d.preRollBars
        passesAsTakes = d.passesAsTakes
    }

    /// Count-in off ↔ its last bar count: the keyboard's Count-in button
    /// (plan 5f-4 Ruling 16). It only changes the next Record.
    public func toggleCountIn() {
        countInBars = countInBars > 0 ? 0 : lastCountIn
    }

    /// A pass is recording.
    public var isRecording: Bool { state?.recording ?? false }

    /// A loop pass is recording: keep and discard apply.
    public var isLoopRecording: Bool { isRecording && (state?.spec?.looped ?? false) }

    /// What Record asks for now.
    public func spec() -> RecordSpecRow {
        let t = document?.editor.layout.transport ?? TransportMarks()
        return RecordSpecRow(
            replace: !t.overdub,
            punch: t.punch ? TickRange(start: t.punchIn, end: t.punchOut) : nil,
            looped: document?.looping ?? false,
            countInBars: countInBars, preRollBars: preRollBars, passesAsTakes: passesAsTakes)
    }

    /// `transport.record`: records the shown sequence, or, while recording,
    /// stops (which finishes the pass).
    public func toggle() -> KeyOutcome {
        guard let m = document else { return .unavailable("No document is open.") }
        // A pass can end by itself with no poll since (Task 15).
        refresh(quiet: true)
        if isRecording {
            do {
                try m.session.stop()
            } catch {
                return Self.refused(error)
            }
            refresh()
            m.poll()
            return .done
        }
        guard let seq = m.selectedSeq else { return .unavailable(TransportCommands.noSequence) }
        guard !m.stepEntry.isOn else { return .refused(Self.stepEntryOn) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        // A punch time typed in the popover but not yet set refuses the
        // pass (controller ruling I5): it must never punch at the old range
        // with a typed time on screen (5d-2 final review I3, re-review N1).
        guard !hasOpenPunchDraft else { return .refused(Self.openDraftMessage) }
        do {
            try m.session.record(seq: seq, spec: spec())
        } catch {
            return Self.refused(error)
        }
        refresh()
        m.poll()
        return .done
    }

    /// `record.keep_pass` (`keep`) or `record.discard_pass`.
    public func press(keep: Bool) -> KeyOutcome {
        guard let m = document else { return .unavailable("No document is open.") }
        guard isLoopRecording else { return .unavailable("Keep and discard act on loop passes while recording.") }
        do {
            if keep { try m.session.keepPass() } else { try m.session.discardPass() }
        } catch {
            return Self.refused(error)
        }
        refresh()
        return .done
    }

    /// Reads what the core is doing (the poll calls this while playing or
    /// recording). `quiet`: a failure keeps the last report and leaves the
    /// status line alone (a command's pre-read, Task 15).
    public func refresh(quiet: Bool = false) {
        guard let m = document else { return }
        do {
            let s = try m.session.recordState()
            if s != state { state = s }
        } catch VizError.Busy {
            return
        } catch {
            if !quiet { m.editor.status = AlertItem(error, title: "").message }
        }
    }

    /// Offers the recordings the project holds that nothing names (spec
    /// §6.5), unless the control channel is on (it answers with
    /// `record.recover`).
    func offerRecoveryIfNeeded(channelOn: Bool = MediaModel.channelIsOn()) {
        guard !channelOn, let m = document else { return }
        offerHeld = false
        recoveryOffer = (try? m.session.recoveredFiles()) ?? []
    }

    /// Whether the window shows the recovery alert.
    public func showsRecoveryAlert(channelOn: Bool) -> Bool {
        !channelOn && !recoveryOffer.isEmpty && !offerHeld
    }

    /// The offer stays after a busy or failed answer; the alert waits for
    /// the next poll (`reofferIfHeld`).
    private var offerHeld = false

    /// Shows a held offer again (the poll calls this).
    func reofferIfHeld() { offerHeld = false }

    /// The alert's text: how many recordings, their names and (pre-flight
    /// M9) where they came from. Only a file whose size was never written
    /// was left by a crash; a finished take that nothing names (undone,
    /// then saved) is "not in the project".
    public var recoveryMessage: String {
        Self.message(for: recoveryOffer)
    }

    static func message(for files: [RecoveredFileRow]) -> String {
        let names = files.map { (($0.path as NSString).lastPathComponent as NSString).deletingPathExtension }
        let one = names.count == 1
        let crashed = files.filter { !$0.sizeKnown && $0.readable }.count
        let text: String
        if crashed == files.count {
            text = "\(one ? "A recording was" : "\(names.count) recordings were") left by VizDAW quitting unexpectedly"
        } else {
            let also = crashed == 0 ? "" : " (\(crashed) left by VizDAW quitting unexpectedly)"
            text = "\(one ? "A recording is" : "\(names.count) recordings are") not in the project\(also)"
        }
        return "\(text): \(names.joined(separator: ", "))."
    }

    /// Answers the offer: import (`true`: a new lane each, one undo step)
    /// or move to the Trash (`false`); `nil` ("Later") leaves them to be
    /// offered at the next open.
    public func answerRecovery(import answer: Bool?) {
        // The alert's dismissal calls this with nil after a button already
        // answered; a held offer (busy or failed) must survive that.
        guard let m = document, let answer else {
            if !offerHeld { recoveryOffer = [] }
            return
        }
        do {
            m.apply(try m.session.recoverRecordings(import: answer))
            recoveryOffer = []
        } catch VizError.Busy {
            // The core is busy: keep the offer and ask again at the next poll.
            offerHeld = true
        } catch {
            offerHeld = true
            m.alert = AlertItem(error, title: "Recovery Failed")
        }
    }

    static func refused(_ error: any Error) -> KeyOutcome {
        if case VizError.Busy(let message) = error { return .busy(message) }
        return .refused(AlertItem(error, title: "").message)
    }
}
