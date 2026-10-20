import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The agent control channel (control spec §4): off unless `--control
/// PATH` or `VIZDAW_CONTROL` names a socket. A 20 Hz main-thread timer
/// takes the calls Rust validated and queued, runs them like the user's
/// menu items, clicks and keys would, and answers. Calls that must wait
/// (a window to appear, posted events to be handled, a `wait`) are kept
/// as deferred answers and checked on every tick.
@MainActor
enum ControlChannel {
    private(set) static var server: ControlServer?
    private static var timer: Timer?
    /// App-level alerts the channel kept from showing (newest last).
    private(set) static var alerts: [AlertItem] = []
    private static var deferred: [Deferred] = []
    /// Which documents' alerts were reported already.
    private static var documentAlerts = ControlAlertLog()
    private static let clock = ContinuousClock()

    /// A call answered later: `step(expired)` answers and returns true
    /// when done (`expired`: its time is up, it must answer now).
    struct Deferred {
        let token: UInt64
        let deadline: ContinuousClock.Instant
        let step: @MainActor (_ expired: Bool) -> Bool
    }

    /// How long a call that started a job waits for it.
    private static var jobWait = ControlJobs.waitMs

    static var active: Bool { server != nil }

    /// Starts listening on `path`; a socket that cannot be made ends the
    /// app (exit status 2), so a launching agent sees why at once.
    static func start(path: String, appTimeoutMs: UInt64 = 60_000, jobWaitMs: UInt64 = ControlJobs.waitMs) {
        jobWait = jobWaitMs
        do {
            server = try ControlServer.start(path: path, appTimeoutMs: appTimeoutMs)
        } catch {
            FileHandle.standardError.write(Data("vizdaw: \(AlertItem(error, title: "Control").message)\n".utf8))
            exit(2)
        }
        FileHandle.standardError.write(Data("vizdaw: control channel on \(path)\n".utf8))
        let t = Timer(timeInterval: 0.05, repeats: true) { _ in MainActor.assumeIsolated { pump() } }
        // Common modes: answers keep coming while a menu or a drag tracks.
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    /// The app quits: pending calls get `shutting_down`, the socket goes.
    static func stop() {
        timer?.invalidate()
        timer = nil
        try? server?.stop()
        server = nil
    }

    /// Keeps an alert the app would have shown (control spec §7).
    static func note(_ item: AlertItem) {
        FileHandle.standardError.write(Data("vizdaw: alert \(item.title): \(item.message)\n".utf8))
        alerts.append(item)
        if alerts.count > 20 { alerts.removeFirst(alerts.count - 20) }
    }

    static func pump() {
        guard let server else { return }
        let now = clock.now
        // Swap the queue out first: a step may queue another deferred step
        // (via `later`), which must survive this tick.
        let running = deferred
        deferred = []
        let kept = running.filter { d in
            guard server.isWaiting(token: d.token) else { return false }
            return !d.step(now >= d.deadline)
        }
        deferred = kept + deferred
        for _ in 0..<8 {
            guard let call = (try? server.nextCall()) ?? nil else { break }
            execute(call, server)
        }
        noteDocumentAlerts()
    }

    /// Reports each document alert once (their sheets do not open while
    /// the channel is on); a call that raised one has already taken it as
    /// its failure.
    static func noteDocumentAlerts() {
        let current = appStore.documents.sorted { $0.key.uuidString < $1.key.uuidString }.compactMap(\.value.alert)
        for a in documentAlerts.unreported(current) { note(a) }
    }

    static func answer(_ server: ControlServer, _ token: UInt64, _ outcome: ControlOutcome) {
        switch outcome {
        case .done(let r): _ = try? server.reply(token: token, result: r)
        case .failed(let f): _ = try? server.fail(token: token, failure: f)
        case .opened(let command, let doc): whenShown(server, token, command: command, doc: doc)
        case .job(let command, let doc, let job):
            // The server's app timeout is for stalls; a job gets its own.
            _ = try? server.extendTimeout(token: token, timeoutMs: jobWait + 5_000)
            whenJobEnds(server, token, command: command, doc: doc, job: job)
        }
    }

    /// Answers once job `job` of `doc` ended (an import or conversion may
    /// take minutes; `jobWait` at most).
    static func whenJobEnds(_ server: ControlServer, _ token: UInt64, command: String, doc: UUID, job: UInt64) {
        later(token, seconds: Double(jobWait) / 1000) { expired in
            guard let m = appStore.model(doc) else {
                _ = try? server.fail(token: token, failure: .of(.noDocument, "the document closed"))
                return true
            }
            guard let j = try? m.session.job(id: job) else {
                _ = try? server.fail(token: token, failure: .of(.failed, "the job is unknown"))
                return true
            }
            switch ControlJobs.answer(j) {
            case .wait:
                if expired {
                    _ = try? server.fail(
                        token: token, failure: .of(.timeout, ControlJobs.expiredMessage(j)))
                }
                return expired
            case .ok(let status):
                m.refreshAfterJob()
                let window = ControlWindows.window(of: doc).map { Int64($0.windowNumber) }
                _ = try? server.reply(
                    token: token,
                    result: .ran(
                        command: command, doc: doc.uuidString, window: window, undoLabel: m.snapshot.undoLabel,
                        status: status))
                return true
            case .failed(let message):
                // A failed import may still have changed nothing; a refresh is harmless.
                m.refreshAfterJob()
                _ = try? server.fail(token: token, failure: .of(.failed, message))
                return true
            }
        }
    }

    static func later(_ token: UInt64, seconds: Double, _ step: @escaping @MainActor (Bool) -> Bool) {
        if !step(false) {
            deferred.append(Deferred(token: token, deadline: clock.now + .seconds(seconds), step: step))
        }
    }

    /// Opens `doc`'s window and answers once it is on screen.
    static func whenShown(_ server: ControlServer, _ token: UInt64, command: String?, doc: UUID) {
        ControlWindows.openDocument(doc)
        later(token, seconds: 30) { expired in
            if let w = ControlWindows.window(of: doc) {
                let r: ControlResult =
                    command.map {
                        .ran(command: $0, doc: doc.uuidString, window: Int64(w.windowNumber), undoLabel: nil, status: nil)
                    } ?? .opened(doc: doc.uuidString, window: Int64(w.windowNumber))
                _ = try? server.reply(token: token, result: r)
                return true
            }
            guard expired else { return false }
            _ = try? server.fail(
                token: token, failure: .of(.timeout, "the new document's window did not appear within 30 s"))
            return true
        }
    }

    static func execute(_ call: ControlCall, _ server: ControlServer) {
        let token = call.token
        if controlHostBusy() {
            answer(server, token, .failed(.of(.busy, "a plugin call is running on the main thread; try again")))
            return
        }
        if staleWindow(call.request) { return answer(server, token, noWindow) }
        switch call.request {
        case .runCommand(let id, let args, let window):
            answer(server, token, ControlCommands.run(
                id, args: args, doc: ControlWindows.document(window), store: appStore, host: AppControlHost.shared))
        case .state(let target, let window):
            if target == .windows {
                answer(server, token, .done(.windows(windows: ControlWindows.list(), alerts: alerts.map {
                    AlertRow(title: $0.title, message: $0.message)
                })))
            } else if let (id, m) = ControlWindows.document(window),
                let r = ControlStates.state(target, id, m)
            {
                answer(server, token, .done(r))
            } else {
                answer(server, token, .failed(.of(.noDocument, "No document is open in that window.")))
            }
        case .tree(let window, let maxDepth):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            let (nodes, truncated) = ControlUI.tree(of: w, maxDepth: Int(maxDepth))
            answer(server, token, .done(.tree(window: Int64(w.windowNumber), nodes: nodes, truncated: truncated)))
        case .click(let window, let at, let button, let count, let modifiers):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let p = ControlUI.point(at, in: w) else { return answer(server, token, noElement) }
            let events = ControlUI.clickEvents(w, at: p, button: button, count: Int(count), modifiers: modifiers)
            send(server, token, events, w, p)
        case .drag(let window, let from, let to, let steps, let modifiers):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let a = ControlUI.point(from, in: w), let b = ControlUI.point(to, in: w) else {
                return answer(server, token, noElement)
            }
            send(server, token, ControlUI.dragEvents(w, from: a, to: b, steps: Int(steps), modifiers: modifiers), w, b)
        case .key(let window, let focus, let strokes):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            var click: [NSEvent] = []
            if let focus {
                guard let p = ControlUI.point(.element(element: focus), in: w) else {
                    return answer(server, token, noElement)
                }
                click = ControlUI.clickEvents(
                    w, at: p, button: .left, count: 1,
                    modifiers: ModifiersRow(shift: false, control: false, option: false, command: false))
            }
            // The focusing click first; the keys once it was handled.
            send(server, token, click, w, .zero) { keys(server, token, strokes, w) }
        case .screenshot(let window, let path, let scale):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let png = WindowSnapshot.png(of: w, scale: scale.map { CGFloat($0) }) else {
                return answer(server, token, .failed(.of(.failed, "the window could not be rendered")))
            }
            do {
                try png.data.write(to: URL(fileURLWithPath: path), options: .atomic)
                answer(server, token, .done(.screenshot(
                    window: Int64(w.windowNumber), path: path, width: UInt32(png.width), height: UInt32(png.height))))
            } catch {
                answer(server, token, .failed(.of(.failed, "cannot write \(path): \(error.localizedDescription)")))
            }
        case .newDemo(let audio):
            let id = audio ? appStore.newDemoAudio() : appStore.newDemo()
            answer(server, token, id.map { .opened(command: nil, doc: $0) }
                ?? .failed(appStore.takeAlert().map(ControlFailure.alert) ?? .of(.failed, "no demo was made")))
        case .open(let path, let answers):
            answer(server, token, ControlOpen.open(path: path, answers: answers, store: appStore))
        case .quit:
            _ = try? server.reply(token: token, result: .quitting)
            // Next turn: the reply is on its way before the app tears down.
            DispatchQueue.main.async { NSApp.terminate(nil) }
        case .wait(let condition, let window, let timeoutMs):
            let start = clock.now
            var last = facts(window)
            later(token, seconds: Double(timeoutMs) / 1000) { expired in
                last = facts(window)
                if controlWaitMet(condition: condition, facts: last) {
                    let ms = (clock.now - start).components
                    let elapsed = UInt64(ms.seconds * 1000) + UInt64(ms.attoseconds / 1_000_000_000_000_000)
                    _ = try? server.reply(
                        token: token, result: .waited(condition: ControlWindows.name(condition), elapsedMs: elapsed))
                    return true
                }
                guard expired else { return false }
                _ = try? server.waitTimedOut(token: token, condition: condition, facts: last, timeoutMs: timeoutMs)
                return true
            }
        }
    }

    /// A window number that names no window is `no_window` for every
    /// command (control spec §5.3), not `no_document`.
    static func staleWindow(_ request: ControlRequest) -> Bool {
        let window: WindowRefRow?
        switch request {
        case .runCommand(_, _, let w), .state(_, let w): window = w
        case .wait(let c, let w, _): window = ControlWindowCheck.waitReadsDocument(c) ? w : nil
        default: return false
        }
        return ControlWindowCheck.namesNoWindow(window) { ControlWindows.window(.number(number: $0)) != nil }
    }

    static let noWindow = ControlOutcome.failed(.of(.noWindow, "No such VizDAW window (see state.get windows)."))
    static let noElement = ControlOutcome.failed(
        .of(.noElement, "The element is gone or changed (read ui.tree again)."))

    /// Posts `events` and, once the app has handled them, answers (or runs
    /// `then`, which answers).
    static func send(
        _ server: ControlServer, _ token: UInt64, _ events: [NSEvent], _ w: NSWindow, _ p: CGPoint,
        then: (@MainActor () -> Void)? = nil
    ) {
        ControlUI.post(events, to: w)
        later(token, seconds: 5) { expired in
            guard expired || !ControlUI.eventsPending else { return false }
            if let then {
                then()
            } else {
                _ = try? server.reply(
                    token: token,
                    result: .input(window: Int64(w.windowNumber), events: UInt32(events.count), x: p.x, y: p.y))
            }
            return true
        }
    }

    /// Key strokes into `w` (control spec §5.5, plan 4f Ruling 5): routed
    /// as the keyboard's are, by the current key bindings. A bound key runs
    /// its command on the window's document (a menu item's too: the
    /// channel has no menus, so a dialog it would open is
    /// `dialog_required`); a keyboard slot plays or enters its sequence; a
    /// text field's plain keys, an unavailable command and an unbound key
    /// go to the window's first responder (typing, canvas keys).
    static func keys(_ server: ControlServer, _ token: UInt64, _ strokes: [KeyStrokeRow], _ w: NSWindow) {
        var sent: UInt32 = 0
        for s in strokes {
            let events = ControlUI.keyEvents(w, [s])
            guard let down = events.first else { continue }
            let (route, doc) = KeyMonitor.route(down, in: w, menus: false)
            switch route {
            case .command(let id):
                let outcome = ControlCommands.run(id, args: [], doc: doc, store: appStore, host: AppControlHost.shared)
                switch outcome {
                // Delivered as the keyboard passes it (final review M-2).
                case .failed(let f) where KeyDispatch.channelDelivers(f.code): break
                case .failed(let f):
                    _ = try? server.fail(token: token, failure: f)
                    return
                case .done, .opened, .job:
                    if case .opened(_, let doc) = outcome { ControlWindows.openDocument(doc) }
                    sent += 1
                    continue
                }
            case .slot:
                if KeyMonitor.perform(route, doc) {
                    sent += 1
                    continue
                }
            case .typing, .pass: break
            }
            ControlUI.deliver(events, to: w)
            sent += UInt32(events.count)
        }
        _ = try? server.reply(token: token, result: .input(window: Int64(w.windowNumber), events: sent, x: 0, y: 0))
    }

    /// What a wait reads now.
    static func facts(_ window: WindowRefRow?) -> WaitFactsRow {
        WaitFactsRow(
            document: ControlWindows.document(window).map { ControlStates.facts($0.model) },
            documents: UInt32(appStore.documents.count),
            windowTitles: ControlWindows.list().filter(\.visible).map(\.title))
    }
}

/// VizDAW's windows for the channel: what each shows, which one a call
/// means, and opening a document's window from outside SwiftUI.
@MainActor
enum ControlWindows {
    final class Tag {
        let kind: WindowKindRow
        let doc: UUID?
        init(_ kind: WindowKindRow, _ doc: UUID?) {
            self.kind = kind
            self.doc = doc
        }
    }

    private static let tags = NSMapTable<NSWindow, Tag>.weakToStrongObjects()
    /// SwiftUI's window opener, taken from the first document window.
    static var openWindow: OpenWindowAction?

    static func tag(_ w: NSWindow, _ kind: WindowKindRow, doc: UUID?) {
        tags.setObject(Tag(kind, doc), forKey: w)
    }

    static func tagOf(_ w: NSWindow) -> Tag {
        if let t = tags.object(forKey: w) { return t }
        if w.title == "Plugin Manager" { return Tag(.pluginManager, nil) }
        if w.identifier?.rawValue.contains("Settings") == true { return Tag(.settings, nil) }
        return Tag(.other, nil)
    }

    /// VizDAW's titled windows, front to back.
    static func windows() -> [NSWindow] {
        NSApp.windows
            .filter { $0.styleMask.contains(.titled) && ($0.isVisible || $0.isMiniaturized) }
            .sorted { $0.orderedIndex < $1.orderedIndex }
    }

    static func list() -> [WindowRow] {
        windows().map { w in
            let t = tagOf(w)
            return WindowRow(
                number: Int64(w.windowNumber), title: w.title, kind: t.kind, doc: t.doc?.uuidString,
                key: w.isKeyWindow, main: w.isMainWindow, visible: w.isVisible, x: w.frame.minX,
                y: w.frame.minY, width: w.frame.width, height: w.frame.height)
        }
    }

    /// The window `ref` names; none: the front document window.
    static func window(_ ref: WindowRefRow?) -> NSWindow? {
        let all = windows()
        switch ref {
        case nil: return all.first { tagOf($0).kind == .document } ?? all.first
        case .key: return all.first { $0.isKeyWindow }
        case .main: return all.first { $0.isMainWindow }
        case .number(let n): return all.first { $0.windowNumber == Int(n) }
        }
    }

    /// The document `ref`'s window shows; none: the front document
    /// window's, else any open document.
    static func document(_ ref: WindowRefRow?) -> (id: UUID, model: DocumentModel)? {
        let id: UUID? =
            if let ref { window(ref).flatMap { tagOf($0).doc } } else {
                window(nil).flatMap { tagOf($0).doc } ?? appStore.documents.keys.sorted { $0.uuidString < $1.uuidString }.first
            }
        return id.flatMap { i in appStore.model(i).map { (i, $0) } }
    }

    /// Document `doc`'s own window, once it is on screen.
    static func window(of doc: UUID) -> NSWindow? {
        windows().first { w in
            let t = tagOf(w)
            return t.kind == .document && t.doc == doc && w.isVisible
        }
    }

    static func openDocument(_ doc: UUID) {
        guard window(of: doc) == nil else { return }
        if let openWindow {
            openWindow(value: doc)
        } else {
            appStore.enqueue(doc)
        }
    }

    static func name(_ c: WaitConditionRow) -> String {
        switch c {
        case .playing: "playing"
        case .stopped: "stopped"
        case .tickAtLeast: "tick_at_least"
        case .documentsAtLeast: "documents_at_least"
        case .windowTitled: "window_titled"
        case .windowGone: "window_gone"
        case .detailKind: "detail_kind"
        }
    }
}

/// Marks the window a SwiftUI view is in (document and mixer windows).
struct WindowTag: NSViewRepresentable {
    let kind: WindowKindRow
    let doc: UUID?

    final class Marker: NSView {
        var kind: WindowKindRow = .other
        var doc: UUID?
        // Inert for the user: clicks and VoiceOver pass through it.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let w = window { ControlWindows.tag(w, kind, doc: doc) }
        }
    }

    func makeNSView(context: Context) -> Marker {
        let m = Marker()
        m.kind = kind
        m.doc = doc
        return m
    }

    func updateNSView(_ m: Marker, context: Context) {
        m.kind = kind
        m.doc = doc
        if let w = m.window { ControlWindows.tag(w, kind, doc: doc) }
    }
}

/// The app's windows and settings for `ControlCommands`.
@MainActor
final class AppControlHost: ControlHost {
    static let shared = AppControlHost()

    func showMixer(_ doc: UUID) { ControlWindows.openWindow?(id: "mixer", value: doc) }
    func showPluginManager() { ControlWindows.openWindow?(id: "plugins") }
    func showMediaPool(_ doc: UUID) { ControlWindows.openWindow?(id: "media-pool", value: doc) }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { EditorWindows.tearOff(appStore, doc: doc, kind: kind) }

    func closeDocument(_ doc: UUID) {
        if let w = ControlWindows.window(of: doc) {
            w.performClose(nil)
        } else {
            appStore.close(doc)
        }
    }

    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }

    func focus(_ field: TransportField, _ doc: UUID) -> Bool {
        guard let w = ControlWindows.window(of: doc),
            let v = TransportFieldsView.find(in: w.contentView?.superview ?? w.contentView)
        else { return false }
        return v.focus(field)
    }
}
