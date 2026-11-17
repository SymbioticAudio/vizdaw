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

extension CoProducerApproval {
    /// Whether the event that pressed Allow is the composer's own (Ruling
    /// 11): a left-mouse-up or key-down whose source process is 0 (the
    /// window server's input) or VizDAW itself. No event (an accessibility
    /// press) or an event another process posted is not. A backstop only:
    /// a program with Accessibility access can still drive the Mac.
    public static func isRealPress(type: NSEvent.EventType?, sourcePid: Int64, ownPid: Int64) -> Bool {
        guard let type, type == .leftMouseUp || type == .keyDown else { return false }
        return sourcePid == 0 || sourcePid == ownPid
    }

    /// `isRealPress` of AppKit's current event, which must also be fresh:
    /// an accessibility action can run while an old click is still
    /// `NSApp.currentEvent`.
    public static func currentEventIsRealPress() -> Bool {
        guard let e = NSApp.currentEvent else { return false }
        guard ProcessInfo.processInfo.systemUptime - e.timestamp < maxPressAge else { return false }
        let source = e.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1
        return isRealPress(type: e.type, sourcePid: source, ownPid: Int64(getpid()))
    }

    /// How old the current event may be to count as the press (seconds).
    public static let maxPressAge: TimeInterval = 1
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
        case .unverified(_, let path, let label): return label ?? URL(fileURLWithPath: path).lastPathComponent
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

    /// Allow, if `realPress`. With the box ticked for an unverified caller
    /// it shows the warning first.
    public func allow(realPress: Bool) {
        guard realPress, step == .ask else { return }
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
    }

    /// The warning's Always Allow Anyway, if `realPress`.
    public func allowAnyway(realPress: Bool) {
        guard realPress, step == .warning else { return }
        _ = respond(.allow(always: true, confirmed: true))
    }
}
