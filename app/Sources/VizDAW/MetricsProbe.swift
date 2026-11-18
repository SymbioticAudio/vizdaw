import AppKit
import Foundation
import Observation
import VizDAWAppModel
import VizDAWCore

/// Where the composer's time goes (plan 8f Task 4), while Settings ›
/// Co-Producer › Count where my time goes is on: a timer samples the window
/// in use (Ruling 3), an in-app event monitor notes only when the last
/// input was, the main menu's tracking sessions are timed, and each
/// document's steps and undos are passed on. Nothing here records a key,
/// a position, a note or a file name. Every recorder call is made on the
/// main actor.
@MainActor
enum MetricsProbe {
    private static var metrics: CoProducerMetrics?
    private static var store: DocumentStore?
    private static var timer: Timer?
    private static var monitor: Any?
    private static var observers: [any NSObjectProtocol] = []
    private static var ticker = MetricsTicker(sampleMs: CoProducerMetrics.sampleMs)
    /// The last key window, for samples while VizDAW is in the background.
    private static weak var lastKey: NSWindow?

    static var running: Bool { timer != nil }

    /// Starts and stops the probe with `metrics.on`, now and at each change.
    static func follow(_ metrics: CoProducerMetrics, store: DocumentStore) {
        withObservationTracking {
            _ = metrics.on
        } onChange: {
            DispatchQueue.main.async { follow(metrics, store: store) }
        }
        if metrics.on { start(metrics, store: store) } else { stop() }
    }

    /// Starts sampling; a second start is a no-op.
    static func start(_ metrics: CoProducerMetrics, store: DocumentStore) {
        guard timer == nil else { return }
        self.metrics = metrics
        self.store = store
        let sampleMs = CoProducerMetrics.timing(environment: ProcessInfo.processInfo.environment).sampleMs
        ticker = MetricsTicker(sampleMs: sampleMs)
        lastKey = NSApp.keyWindow
        // The grid starts now, before the timer's first tick, so no tick
        // comes before a due time.
        tick()
        let t = Timer(timeInterval: Double(sampleMs) / 1000, repeats: true) { _ in
            MainActor.assumeIsolated { tick() }
        }
        // Common modes: keeps sampling while a menu or a drag tracks.
        RunLoop.main.add(t, forMode: .common)
        timer = t
        // Only that input happened; the event goes on unchanged.
        monitor = NSEvent.addLocalMonitorForEvents(
            matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel]
        ) { e in
            MainActor.assumeIsolated { ticker.input(at: now().ms) }
            return e
        }
        let center = NotificationCenter.default
        observers.append(
            center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main) { note in
                nonisolated(unsafe) let n = note
                MainActor.assumeIsolated {
                    if let w = n.object as? NSWindow { lastKey = w }
                }
            })
        observers.append(
            center.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: .main) { note in
                nonisolated(unsafe) let n = note
                MainActor.assumeIsolated {
                    guard isMainMenu(n.object) else { return }
                    ticker.menuBegan(at: now().ms)
                }
            })
        observers.append(
            center.addObserver(forName: NSMenu.didSendActionNotification, object: nil, queue: .main) { note in
                nonisolated(unsafe) let n = note
                MainActor.assumeIsolated {
                    guard let item = n.userInfo?["MenuItem"] as? NSMenuItem else { return }
                    ticker.menuChose(item.title)
                }
            })
        observers.append(
            center.addObserver(forName: NSMenu.didEndTrackingNotification, object: nil, queue: .main) { note in
                nonisolated(unsafe) let n = note
                MainActor.assumeIsolated {
                    guard isMainMenu(n.object) else { return }
                    let end = now()
                    // The chosen item's action is sent after tracking ends,
                    // so the session is closed on the next turn.
                    DispatchQueue.main.async {
                        MainActor.assumeIsolated { menuEnded(end) }
                    }
                }
            })
        store.stepObserver = { id, event in step(id, event) }
    }

    /// Stops sampling: the timer, the monitor and every observer go.
    static func stop() {
        timer?.invalidate()
        timer = nil
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        for o in observers { NotificationCenter.default.removeObserver(o) }
        observers = []
        store?.stepObserver = nil
        store = nil
        metrics = nil
        lastKey = nil
    }

    private static func now() -> MetricsClockRow { CoProducerMetrics.clock(Date()) }

    private static func isMainMenu(_ object: Any?) -> Bool {
        guard let menu = object as? NSMenu, let main = NSApp.mainMenu else { return false }
        return menu === main
    }

    /// One sample by Ruling 3, when due.
    private static func tick() {
        guard let recorder = metrics?.recorder, let store else { return }
        let clock = now()
        guard ticker.due(at: clock.ms) else { return }
        let active = NSApp.isActive
        let window = active ? NSApp.keyWindow : lastKey
        if active, let window { lastKey = window }
        let tag = window.map(ControlWindows.tagOf)
        let windowDoc = tag?.doc.flatMap(store.model)
        let recordingDoc = store.documents.values.first { $0.record.isRecording }
        let songDoc = recordingDoc ?? windowDoc ?? store.frontDocument
        let detail: DetailEditorKind? =
            if tag?.kind == .editor, let window {
                EditorWindows.kind(of: window)
            } else {
                windowDoc?.editor.layout.detail?.kind
            }
        let context = MetricsContext(
            recording: recordingDoc != nil, playing: (windowDoc ?? songDoc)?.isPlaying ?? false,
            appActive: active, secondsSinceInput: ticker.secondsSinceInput(at: clock.ms), window: tag?.kind,
            detail: detail, tool: windowDoc?.editor.tool ?? .arrow, stepEntry: windowDoc?.stepEntry.isOn ?? false)
        recorder.sample(clock: clock, category: MetricsCategory.classify(context), song: songDoc.map(song))
    }

    /// The song row: its project ID and its own name, never a path.
    private static func song(_ m: DocumentModel) -> MetricsSongRow {
        MetricsSongRow(id: (try? m.session.projectId()) ?? nil, name: m.snapshot.title)
    }

    /// A document's new step or undo (redos are not counted).
    private static func step(_ id: UUID, _ event: StepEvent) {
        guard let recorder = metrics?.recorder, let m = store?.model(id) else { return }
        switch event {
        case .newStep(let label): recorder.noteStep(clock: now(), song: song(m), label: label)
        case .undo: recorder.noteUndo(clock: now(), song: song(m))
        case .redo: break
        }
    }

    /// A main-menu session ended; the recorder keeps it only when long.
    private static func menuEnded(_ end: MetricsClockRow) {
        guard let r = ticker.menuEnded(at: end.ms), let recorder = metrics?.recorder else { return }
        recorder.noteMenu(clock: end, durationMs: r.durationMs, chosen: r.chosen)
    }
}
