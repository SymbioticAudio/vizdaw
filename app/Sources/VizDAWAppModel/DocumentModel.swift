import Foundation
import Observation
import VizDAWCore

/// One window's document: snapshot, selection, lanes and transport.
/// All state comes from the session; intents call it and refresh.
@Observable
@MainActor
public final class DocumentModel {
    public let session: any SessionProtocol
    public private(set) var snapshot: DocSnapshot
    public private(set) var selectedSeq: UInt64?
    public private(set) var lanes: [LaneRow] = []
    public private(set) var transport: TransportState
    public private(set) var looping = false
    /// The sequence whose name is being edited in the sidebar.
    public private(set) var renamingSeq: UInt64?
    /// The edited name (view state lives here: the Command Line Tools
    /// cannot expand SwiftUI's `@State` macro).
    public var renameDraft = ""
    /// The fault popover is open.
    public var showsFaults = false
    public var alert: AlertItem?
    /// Version and selection the current `lanes` were fetched for.
    private var lanesKey: LanesKey?
    /// How often `lanes` were fetched (tests check refetch rules).
    public private(set) var laneFetches = 0
    /// Plugins that failed to load or refused their state (newest last).
    public private(set) var pluginProblems: [String] = []
    /// Bumped when plugin slots change (loaded, failed, restarted): views
    /// that show slots refetch.
    public private(set) var slotsVersion = 0
    /// Slots whose parameters changed outside a gesture (their windows
    /// refetch), cleared by whoever handles them.
    public var paramsChanged: Set<UInt64> = []

    private struct LanesKey: Equatable {
        let version: UInt64
        let seq: UInt64?
    }

    public init(session: any SessionProtocol) throws {
        self.session = session
        snapshot = try session.snapshot()
        // Busy (an AU v3 editor opening in a nested run loop): start
        // stopped at the beginning; the next poll reads the real state.
        do {
            transport = try session.transport()
        } catch VizError.Busy {
            transport = Self.stopped
        }
        apply(snapshot)
    }

    /// The transport shown until the first successful read.
    static let stopped = TransportState(
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)

    // MARK: Derived state

    public var topLevel: [SeqRow] { snapshot.sequences.filter(\.topLevel) }
    public var referenced: [SeqRow] { snapshot.sequences.filter { !$0.topLevel } }
    public var windowTitle: String { Formatting.windowTitle(snapshot) }
    public var undoTitle: String { snapshot.undoLabel.map { "Undo \($0)" } ?? "Undo" }
    public var redoTitle: String { snapshot.redoLabel.map { "Redo \($0)" } ?? "Redo" }
    public var canUndo: Bool { snapshot.undoLabel != nil }
    public var canRedo: Bool { snapshot.redoLabel != nil }
    public var isPlaying: Bool { transport.playing }
    /// Transport keys (Space, Return) are off while a name is edited, so
    /// typing a space types a space.
    public var transportKeysEnabled: Bool { renamingSeq == nil }
    public var canPlay: Bool { selectedSeq != nil || isPlaying }
    /// 33 ms while playing, 250 ms when stopped (to catch the end).
    public var pollInterval: Duration { isPlaying ? .milliseconds(33) : .milliseconds(250) }
    public var hasFaults: Bool { !transport.faults.isEmpty }
    public var hasReport: Bool { !transport.report.isEmpty || !pluginProblems.isEmpty }
    /// Every line of the fault popover: faults, compile messages, plugin
    /// problems.
    public var messages: [String] { transport.faults + transport.report + pluginProblems }
    public var selectedName: String? {
        snapshot.sequences.first { $0.id == selectedSeq }?.name
    }

    // MARK: Intents

    public func select(_ seq: UInt64?) {
        guard seq != selectedSeq else { return }
        selectedSeq = seq
        refreshLanes()
    }

    public func beginRename(_ seq: UInt64) {
        renameDraft = snapshot.sequences.first { $0.id == seq }?.name ?? ""
        renamingSeq = seq
    }

    public func cancelRename() { renamingSeq = nil }

    /// Renames the sequence being edited; a blank or unchanged name
    /// just ends editing.
    public func commitRename(_ name: String) {
        guard let seq = renamingSeq else { return }
        renamingSeq = nil
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let current = snapshot.sequences.first { $0.id == seq }?.name
        guard !trimmed.isEmpty, trimmed != current else { return }
        rename(seq, to: trimmed)
    }

    public func rename(_ seq: UInt64, to name: String) {
        command("Rename Failed") { try $0.renameSequence(seq: seq, name: name) }
    }

    public func setMute(lane: UInt64, _ mute: Bool) {
        guard let seq = selectedSeq else { return }
        command("Mute Failed") { try $0.setLaneMute(seq: seq, lane: lane, mute: mute) }
    }

    public func toggleMute(lane: UInt64) {
        guard let row = lanes.first(where: { $0.id == lane }) else { return }
        setMute(lane: lane, !row.mute)
    }

    /// Undo and Redo do nothing while the plugin host is busy (an AU v3
    /// editor opening): the core refuses before touching history.
    public func undo() { command("Undo Failed", quietWhenBusy: true) { try $0.undo() } }

    public func redo() { command("Redo Failed", quietWhenBusy: true) { try $0.redo() } }

    public func playStop() {
        do {
            if isPlaying {
                try session.stop()
            } else if let seq = selectedSeq {
                try session.play(seq: seq)
            }
        } catch {
            alert = AlertItem(error, title: isPlaying ? "Stop Failed" : "Play Failed")
        }
        poll()
    }

    public func toggleLoop() {
        do {
            try session.setLoop(on: !looping)
            looping.toggle()
        } catch {
            alert = AlertItem(error, title: "Loop Failed")
        }
    }

    public func returnToStart() {
        do {
            try session.locate(tick: 0)
        } catch {
            alert = AlertItem(error, title: "Locate Failed")
        }
        poll()
    }

    /// Reads the transport. A failure shows one alert, not one per poll.
    /// `Busy` (an AU v3 editor opening in a nested run loop) keeps the last
    /// transport, silently; the next poll tries again.
    public func poll() {
        do {
            transport = try session.transport()
        } catch VizError.Busy {
            return
        } catch {
            if alert == nil { alert = AlertItem(error, title: "Transport Failed") }
        }
    }

    /// The plugin timer's step (about 60 times a second, spec §5): plugin
    /// loads finish, plugin-UI gestures become undo steps. Failures go to
    /// the fault popover, not to alerts.
    public func idle() {
        guard let u = try? session.idle() else { return }
        if let s = u.snapshot { apply(s) }
        if u.slotsChanged { slotsVersion += 1 }
        if !u.paramsChanged.isEmpty { paramsChanged.formUnion(u.paramsChanged) }
        if !u.problems.isEmpty {
            pluginProblems.append(contentsOf: u.problems)
            if pluginProblems.count > 50 { pluginProblems.removeFirst(pluginProblems.count - 50) }
        }
    }

    /// True when Save needs a path first (untitled).
    public var needsSaveAs: Bool { ((try? session.path()) ?? nil) == nil }

    public func save() {
        perform("Save Failed") { try $0.save() }
    }

    public func saveAs(path: String) {
        perform("Save Failed") { try $0.saveAs(path: path) }
    }

    // MARK: Plumbing

    private func command(
        _ title: String, quietWhenBusy: Bool = false,
        _ body: (any SessionProtocol) throws -> DocSnapshot
    ) {
        do {
            apply(try body(session))
        } catch VizError.Busy where quietWhenBusy {
            return
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    private func perform(_ title: String, _ body: (any SessionProtocol) throws -> Void) {
        do {
            try body(session)
            apply(try session.snapshot())
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    /// Takes a new snapshot: keeps the selection if the sequence still
    /// exists (else the first top-level one), refetches lanes if needed.
    func apply(_ s: DocSnapshot) {
        snapshot = s
        if selectedSeq.map({ id in !s.sequences.contains { $0.id == id } }) ?? true {
            selectedSeq = s.sequences.first(where: \.topLevel)?.id ?? s.sequences.first?.id
        }
        if let r = renamingSeq, !s.sequences.contains(where: { $0.id == r }) {
            renamingSeq = nil
        }
        refreshLanes()
    }

    private func refreshLanes() {
        let key = LanesKey(version: snapshot.version, seq: selectedSeq)
        guard key != lanesKey else { return }
        lanesKey = key
        guard let seq = selectedSeq else {
            lanes = []
            return
        }
        do {
            laneFetches += 1
            lanes = try session.lanes(seq: seq)
        } catch {
            lanes = []
            alert = AlertItem(error, title: "Lanes Unavailable")
        }
    }
}
