import AppKit
import Foundation
import Observation
import VizDAWCore

/// The composer's answer on VizDAW's approval sheet (plan 8b Ruling 11).
/// `always`: the Always allow box was ticked; `confirmed`: Always Allow
/// Anyway on the warning an unverified caller gets first.
public enum ApprovalAnswer: Equatable, Sendable {
    case allow(always: Bool, confirmed: Bool)
    case deny
}

/// What VizDAW asks the composer about.
public struct PendingApproval: Equatable {
    public let doc: UUID
    public let identity: ClientIdentity
    /// What the caller calls itself (`client`): shown, never trusted.
    public let label: String?
    /// The preview's items.
    public let items: [ChangeItemRow]

    public init(doc: UUID, identity: ClientIdentity, label: String?, items: [ChangeItemRow]) {
        self.doc = doc
        self.identity = identity
        self.label = label
        self.items = items
    }
}

/// Shows VizDAW's approval sheet (plan 8b Ruling 11). The app's approver
/// asks for the composer's attention while `DocumentModel.coProducerApproval`
/// presents the sheet, whose buttons answer through the same handler;
/// tests use `FakeApprover`.
@MainActor
public protocol CoProducerApprover {
    /// Opens the sheet for `pending`; `done` takes the composer's answer.
    /// An answer VizDAW refuses (Always allow before the warning was
    /// confirmed, or for a caller it cannot name) leaves the sheet open,
    /// so `done` may be called again until one is taken.
    func ask(_ pending: PendingApproval, done: @escaping @MainActor (ApprovalAnswer) -> Void)
    /// The request ended without the composer (no answer in time, the song
    /// changed, the caller left, the switch went off, the document closed).
    func cancel(doc: UUID)
}

/// What the press check reads about the event that pressed Allow (plan 8b
/// Task 6 hardening): described, so the decision is a pure function.
public struct PressFacts: Equatable, Sendable {
    /// The event's type; nil when there is no event (an accessibility press).
    public let type: NSEvent.EventType?
    /// A key event's key code.
    public let keyCode: UInt16?
    /// The event's window is the sheet's own.
    public let inSheetWindow: Bool
    /// `.eventSourceUnixProcessID`; -1 when the event has no source.
    public let sourcePid: Int64
    public let ownPid: Int64
    /// Seconds since the event happened.
    public let age: TimeInterval

    public init(
        type: NSEvent.EventType?, keyCode: UInt16?, inSheetWindow: Bool, sourcePid: Int64, ownPid: Int64,
        age: TimeInterval
    ) {
        self.type = type
        self.keyCode = keyCode
        self.inSheetWindow = inSheetWindow
        self.sourcePid = sourcePid
        self.ownPid = ownPid
        self.age = age
    }
}

extension CoProducerApproval {
    /// Space and Return, the keys that press a focused button.
    public static let pressKeys: Set<UInt16> = [49, 36]

    /// How old the press's event may be, in seconds: an accessibility
    /// action can run while an older click is still `NSApp.currentEvent`.
    public static let maxPressAge: TimeInterval = 3

    /// Whether the event that pressed Allow is the composer's own (Ruling
    /// 11, hardened after review): a left-mouse-up, or a key-down or
    /// key-up of Space or Return, in the sheet's own window, from the
    /// window server's input (pid 0) or VizDAW itself, and fresh. No event
    /// (an accessibility press), another process's event, another window's
    /// or another key is refused. Never accepted by session or uid, and
    /// never accepted with a log line instead: it fails closed. A backstop
    /// only: a program with Accessibility access can still drive the Mac.
    public static func isRealPress(_ p: PressFacts) -> Bool {
        guard let type = p.type, p.inSheetWindow, p.age >= 0, p.age < maxPressAge else { return false }
        guard p.sourcePid == 0 || p.sourcePid == p.ownPid else { return false }
        switch type {
        case .leftMouseUp: return true
        case .keyDown, .keyUp: return p.keyCode.map(pressKeys.contains) ?? false
        default: return false
        }
    }

    /// AppKit's current event, described for `isRealPress`, against the
    /// sheet's `window`.
    public static func currentPress(window: NSWindow?) -> PressFacts {
        let e = NSApp.currentEvent
        let isKey = e.map { $0.type == .keyDown || $0.type == .keyUp } ?? false
        return PressFacts(
            type: e?.type, keyCode: isKey ? e?.keyCode : nil,
            inSheetWindow: e?.window != nil && e?.window === window,
            sourcePid: e?.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1, ownPid: Int64(getpid()),
            age: e.map { ProcessInfo.processInfo.systemUptime - $0.timestamp } ?? .infinity)
    }
}

/// VizDAW's approval sheet's state (Ruling 11): its text, the Always allow
/// box, and the warning step for a caller VizDAW cannot verify. It answers
/// through `respond`, the commands' handler.
@Observable
@MainActor
public final class CoProducerApprovalSheetModel: Identifiable {
    public enum Step: Equatable, Sendable { case ask, warning }

    public let id = UUID()
    public let pending: PendingApproval
    /// The song's name.
    public let song: String
    public internal(set) var step: Step = .ask
    /// The Always allow box; unticked at first.
    public var always = false
    /// Shown after a press the check refused, until the next step.
    public private(set) var refusedPress: String?
    /// The window the sheet is in, for the press check (set by the view).
    @ObservationIgnored public weak var window: NSWindow?

    public static let refusedLine = "That press didn't come from your mouse or keyboard. Click Allow yourself."
    @ObservationIgnored let respond: @MainActor (ApprovalAnswer) -> String?

    /// The most item lines the sheet lists.
    public static let maxLines = 40

    init(pending: PendingApproval, song: String, respond: @escaping @MainActor (ApprovalAnswer) -> String?) {
        self.pending = pending
        self.song = song
        self.respond = respond
    }

    public let title = "Allow Co-Producer to Change Your Song?"
    public let warning =
        "VizDAW can't check which program this is. Any program you run from Terminal could use this approval."
    public let unknownLine = "VizDAW cannot confirm which app is asking, so it asks every time."

    /// The asking app as the sheet names it: a known app's name, else the
    /// label, else the host path's last part.
    public var app: String {
        Self.appName(pending)
    }

    nonisolated static func appName(_ p: PendingApproval) -> String {
        switch p.identity {
        case .known(_, _, _, let name): return name
        case .unverified(_, let path, let label): return label ?? shown(URL(fileURLWithPath: path).lastPathComponent)
        case .unknown: return p.label ?? "An app VizDAW cannot identify"
        }
    }

    /// "<App> asks…", or the unidentified sentence for anyone else.
    public var message: String {
        if case .known(_, _, _, let name) = pending.identity {
            return "\(name) asks to make these changes to \"\(song)\":"
        }
        let calls = pending.label.map { " (it calls itself \"\($0)\")" } ?? ""
        return "An app VizDAW cannot identify\(calls) asks to make these changes to \"\(song)\":"
    }

    /// The preview's items, at most 40, then "and N more".
    public var lines: [String] {
        let all = pending.items.map { "\($0.thing): \($0.change)" }
        guard all.count > Self.maxLines else { return all }
        return Array(all.prefix(Self.maxLines)) + ["and \(all.count - Self.maxLines) more"]
    }

    /// Whether the sheet offers Always allow: only for a caller with a key.
    public var offersAlways: Bool { pending.identity.key != nil }
    public var verifies: Bool {
        if case .known = pending.identity { return true }
        return false
    }
    public var alwaysLabel: String { "Always allow \(app)" }
    public var alwaysLine: String {
        "Anything \(app) runs can then change your songs without VizDAW asking. You can stop this in Settings › Co-Producer."
    }

    /// Text a host path or a stored name becomes on screen: controls, bidi
    /// controls and marks, and line and paragraph separators become a
    /// space, as 8a's `diff::shown_name` does.
    public nonisolated static func shown(_ text: String) -> String {
        String(String.UnicodeScalarView(text.unicodeScalars.map { unsafe($0) ? " " : $0 }))
    }

    /// `diff::shown_name`'s refused characters.
    public nonisolated static func unsafe(_ c: Unicode.Scalar) -> Bool {
        switch c.value {
        case 0x00...0x1F, 0x7F...0x9F, 0x061C, 0x200E, 0x200F, 0x202A...0x202E, 0x2066...0x2069, 0x2028, 0x2029:
            return true
        default: return false
        }
    }

    /// Allow, if `realPress`; else the refusal line. With the box ticked
    /// for an unverified caller it shows the warning first.
    public func allow(realPress: Bool) {
        guard step == .ask else { return }
        guard realPress else {
            refusedPress = Self.refusedLine
            return
        }
        refusedPress = nil
        let always = offersAlways && self.always
        if always, !verifies {
            step = .warning
            return
        }
        _ = respond(.allow(always: always, confirmed: false))
    }

    /// Don't Allow (the default button and Esc).
    public func deny() { _ = respond(.deny) }

    /// The warning's Cancel (its default and Esc): back to the sheet, the
    /// box unticked, nothing saved.
    public func cancelWarning() {
        step = .ask
        always = false
        refusedPress = nil
    }

    /// The warning's Always Allow Anyway, if `realPress`; else the
    /// refusal line.
    public func allowAnyway(realPress: Bool) {
        guard step == .warning else { return }
        guard realPress else {
            refusedPress = Self.refusedLine
            return
        }
        refusedPress = nil
        _ = respond(.allow(always: true, confirmed: true))
    }
}
