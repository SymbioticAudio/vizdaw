import Foundation
import Observation
import VizDAWCore

/// Recording in a document (plan 5c Task 13; sub-project 5 spec §6.4,
/// §6.5): what Record asks the core for, what the core reports while it
/// records, and the crash-recovery offer. Replace, punch and loop come
/// from the transport (overdub, the punch fields, Loop); count-in,
/// pre-roll and passes-as-takes are kept here for plan 5d's record
/// popover (and `record.set`).
@Observable
@MainActor
public final class RecordModel {
    @ObservationIgnored weak var document: DocumentModel?
    /// Bars of count-in (0–4; the core refuses more than 0 until plan 5d
    /// plays the click).
    public var countInBars: UInt8 = 0
    /// Bars of pre-roll (0–8).
    public var preRollBars: UInt8 = 0
    /// Kept loop passes become takes.
    public var passesAsTakes = true
    /// What the core last reported.
    public private(set) var state: RecordStateRow?
    /// Recordings found in the project and not yet answered: crash
    /// leftovers and takes nothing names (never offered under the control
    /// channel).
    public private(set) var recoveryOffer: [RecoveredFileRow] = []

    /// Why Record is refused while step entry is on (spec §6.4).
    public static let stepEntryOn = "Step entry is on: turn it off to record."

    init(document: DocumentModel) { self.document = document }

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
