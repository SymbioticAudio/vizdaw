import Foundation
import Observation
import VizDAWCore

/// MIDI learn for one document (plan 5f-1 Task 11; Rulings 7, 8, 11, 18):
/// the "MIDI Learn" items of the mixer, plugin windows and the Strip
/// Chart, Settings › Keys "Learn MIDI", and the MIDI Learn Maps sheet.
///
/// A learn waits in the core until a control moves, it is cancelled or
/// another learn starts (Ruling 7); while it waits the banner shows. The
/// project's maps are this document's (`Session.learnMaps`); the waiting
/// learn and command maps are process-wide, so they go through
/// `SurfaceCore` (pre-flight M25).
@MainActor @Observable public final class LearnModel {
    /// "Waiting for MIDI… move a control (Esc cancels)".
    nonisolated public static let waitingText = "Waiting for MIDI… move a control (Esc cancels)"
    /// The banner's text (final review R2, T11 m9): what the learn waits
    /// for when known, and the project when given (two or more are open),
    /// else `waitingText`.
    nonisolated public static func waiting(project: String?, for what: String? = nil) -> String {
        guard project != nil || what != nil else { return waitingText }
        let forText = what.map { " for \($0)" } ?? ""
        let inText = project.map { " in “\($0)”" } ?? ""
        return "Waiting for MIDI\(forText)\(inText)… move a control (Esc cancels)"
    }

    /// The banner for a window, naming `project` (nil with one project
    /// open) and what the core says the learn waits for; nil when not
    /// learning.
    public func bannerText(project: String?) -> String? {
        guard banner != nil else { return nil }
        return Self.waiting(project: project, for: core.learnState().waiting)
    }

    /// The title of every refusal.
    public static let failedTitle = "MIDI Learn Failed"

    /// Weak: the document owns this model (`DocumentModel.learn`).
    @ObservationIgnored private weak var document: DocumentModel?
    @ObservationIgnored private let session: any SessionProtocol
    @ObservationIgnored private let core: any SurfaceCore

    /// "Waiting for MIDI… move a control (Esc cancels)"; nil when not learning.
    public private(set) var banner: String?
    public private(set) var maps: [LearnMapRow] = []
    /// The names the sheet shows targets by, read when the maps change.
    public private(set) var names = StripNames()
    public var alert: AlertItem?
    /// The MIDI Learn Maps sheet is open (it polls while open).
    public var showsMaps = false {
        didSet { if showsMaps && !oldValue { refreshMaps() } }
    }

    public init(document: DocumentModel, core: any SurfaceCore) {
        self.document = document
        self.session = document.session
        self.core = core
    }

    /// The ticker calls `poll()` only while this holds.
    public var isActive: Bool { banner != nil || showsMaps }

    /// Learns the next control for `t` (a fader, pan, mute, solo, send or
    /// plugin parameter of this project).
    public func learn(_ t: AutomationTargetRow) {
        start { try self.session.learnTarget(target: t) }
    }

    /// Learns the next control for command `id` (Ruling 8: a command a key
    /// can run). The learn completes in this document's session, which
    /// takes the surfaces' focus (Ruling 11).
    public func learnCommand(_ id: String) {
        start {
            try self.session.focusSurfaces()
            try self.core.learnCommand(id)
        }
    }

    private func start(_ body: () throws -> Void) {
        do {
            try body()
            banner = Self.waitingText
        } catch {
            banner = nil
            alert = AlertItem(error, title: Self.failedTitle)
        }
    }

    /// Esc: the waiting learn ends with nothing learned.
    public func cancel() {
        core.cancelLearn()
        banner = nil
    }

    /// The 60 Hz ticker, while learning or the sheet is open: the banner
    /// goes once the core no longer waits, and the maps follow the
    /// project (a learn landed, undo, redo).
    public func poll() {
        if banner != nil && core.learnState().waiting == nil { banner = nil }
        refreshMaps()
    }

    /// Rereads the project's maps; unchanged maps are not reassigned.
    public func refreshMaps() {
        guard let m = try? session.learnMaps() else { return }
        guard m != maps else { return }
        maps = m
        if let n = try? AutomationTargets.load(session).names, n != names { names = n }
    }

    public func setMode(_ m: LearnMapRow, _ mode: LearnModeRow) {
        edit { try self.session.setLearnMode(source: m.source, mode: mode) }
    }

    /// Ruling 18; does nothing for a map whose kind or mode does not pick up.
    public func setPickup(_ m: LearnMapRow, _ on: Bool) {
        guard m.picksUp else { return }
        edit { try self.session.setLearnPickup(source: m.source, on: on) }
    }

    public func remove(_ m: LearnMapRow) {
        edit { try self.session.removeLearn(source: m.source) }
    }

    /// One undo step ("MIDI Learn", "Remove MIDI Learn"); a refusal is an alert.
    private func edit(_ body: () throws -> DocSnapshot) {
        do {
            let s = try body()
            document?.apply(s)
        } catch {
            alert = AlertItem(error, title: Self.failedTitle)
        }
        refreshMaps()
    }

    /// "Absolute", "Relative (2's complement)", "Relative (binary offset)".
    nonisolated public static func modeTitle(_ m: LearnModeRow) -> String {
        switch m {
        case .absolute: "Absolute"
        case .relative2sComplement: "Relative (2's complement)"
        case .relativeBinOffset: "Relative (binary offset)"
        }
    }

    /// Every mode, in the picker's order.
    nonisolated public static let modes: [LearnModeRow] = [.absolute, .relative2sComplement, .relativeBinOffset]

    /// "The control takes over only once it reaches the current value".
    nonisolated public static let pickupHelp = "The control takes over only once it reaches the current value"

    /// What a map drives, as the sheet shows it: the target's name, or
    /// "(missing)" once the target is gone (Ruling 6).
    public func targetText(_ m: LearnMapRow) -> String {
        m.missing ? "(missing)" : names.title(AutoTarget(m.target))
    }
}
