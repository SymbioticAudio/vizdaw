import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The one store of open documents (main actor).
@MainActor let appStore = DocumentStore.live()

/// The app's plugin catalog (main actor).
@MainActor let appPlugins = AppPlugins(open: { try PluginCatalog.open(config: $0) })

/// The Plugin Manager window's model.
@MainActor let pluginManager = PluginManagerModel(plugins: appPlugins)

@main
struct VizDAWApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        appStore.willClose = { PluginWindows.closeDocument($0) }
        appStore.recovery = Recovery(
            check: { try recoveryCheck(path: $0) },
            openWith: { try Session.openWith(path: $0, options: $1) },
            disable: { plugins in
                plugins.map { appPlugins.setEnabled(PluginKey($0), false) }.allSatisfy { $0 }
            },
            shiftHeld: { NSEvent.modifierFlags.contains(.shift) },
            questions: RecoveryAlerts())
        // The catalog and support folder exist before any project can open
        // (launch arguments, Finder opens), so "Disable ‹plugin›" works.
        LaunchArguments.parseFlags(CommandLine.arguments)
        AppSetup.openCatalog()
        LaunchArguments.apply(CommandLine.arguments, to: appStore)
    }

    var body: some Scene {
        WindowGroup(for: UUID.self) { $id in
            DocumentWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 980, height: 620)
        .commands { AppCommands(store: appStore) }
        WindowGroup("Mixer", id: "mixer", for: UUID.self) { $id in
            MixerWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 760, height: 560)
        Window("Plugin Manager", id: "plugins") {
            PluginManagerView(model: pluginManager)
        }
    }
}

/// Brings the app to the front under `swift run` (no bundle) and opens
/// packages handed over by Finder.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
        StoreAlerts.watch(appStore)
        AppSetup.startScan()
        PluginTicker.start(appStore)
    }

    func applicationWillTerminate(_ notification: Notification) {
        // A clean quit leaves no crash markers.
        MainActor.assumeIsolated {
            appStore.closeAll()
            // Stop the engine so no plugin host is dropped while it renders.
            try? shutdown()
        }
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            appStore.enqueue(appStore.open(path: url.path))
        }
    }
}

/// Where VizDAW keeps its files and finds its scanner (Plan Ruling 21).
@MainActor
enum AppSetup {
    /// `VIZDAW_SUPPORT_DIR`, else `~/Library/Application Support/VizDAW`;
    /// `--smoke` runs use a fresh temporary folder (never the user's).
    static var supportDir: URL {
        if LaunchArguments.smokeRun {
            return FileManager.default.temporaryDirectory
                .appendingPathComponent("vizdaw-smoke-\(ProcessInfo.processInfo.processIdentifier)")
        }
        if let d = ProcessInfo.processInfo.environment["VIZDAW_SUPPORT_DIR"] {
            return URL(fileURLWithPath: d)
        }
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("VizDAW")
    }

    /// `VIZDAW_SCANNER` (scripts/swift.sh sets it), else `vizdaw-scan` next
    /// to the app's executable (the bundle).
    static var scanner: String {
        if let s = ProcessInfo.processInfo.environment["VIZDAW_SCANNER"] { return s }
        let exe = Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])
        return exe.deletingLastPathComponent().appendingPathComponent("vizdaw-scan").path
    }

    /// Opens the catalog and scans new or changed plugins (not in `--smoke`
    /// or `--no-scan` runs).
    static func openCatalog() {
        let dir = supportDir
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? setSupportDir(path: dir.path)
        appPlugins.start(config: AppPlugins.config(supportDir: dir, scanner: scanner), scan: false)
    }

    /// After launch: the scan and the catalog's alerts.
    static func startScan() {
        if !LaunchArguments.smokeRun && !LaunchArguments.noScan { appPlugins.scanNew() }
        PluginAlerts.watch(appPlugins)
    }
}

/// Catalog problems are app-level: shown as modal alerts.
@MainActor
enum PluginAlerts {
    static func watch(_ plugins: AppPlugins) {
        withObservationTracking {
            _ = plugins.alert
        } onChange: {
            DispatchQueue.main.async { watch(plugins) }
        }
        DispatchQueue.main.async {
            guard let item = plugins.alert else { return }
            plugins.alert = nil
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = item.title
            alert.informativeText = item.message
            alert.addButton(withTitle: "OK")
            alert.runModal()
        }
    }
}

/// The main-thread plugin timer (spec §5 `host_idle`, 60 Hz): every open
/// document's plugins load, run their callbacks and report gestures here;
/// a running scan's progress is read a few times a second.
@MainActor
enum PluginTicker {
    private static var timer: Timer?
    private static var ticks = 0

    static func start(_ store: DocumentStore) {
        guard timer == nil else { return }
        let t = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
            MainActor.assumeIsolated {
                for model in store.documents.values { model.idle() }
                PluginWindows.tick(store)
                ticks += 1
                if ticks % 15 == 0 { appPlugins.poll() }
            }
        }
        // Common modes: keeps ticking while a menu or a drag tracks.
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }
}

/// `VizDAW [--demo] [--smoke] [path.vizdaw ...]`: documents for the first
/// windows. `--smoke` (implies `--demo`) plays the first window for 3 s,
/// prints the transport and exits 0 if the position advanced, else 1.
enum LaunchArguments {
    @MainActor static var smoke = false
    /// This is a `--smoke` run (stays true after the run started).
    @MainActor static var smokeRun = false
    /// `--no-scan`: do not scan plugins at launch.
    @MainActor static var noScan = false

    /// The flags that decide the support folder and the scan.
    @MainActor static func parseFlags(_ args: [String]) {
        for arg in args.dropFirst() {
            if arg == "--no-scan" { noScan = true }
            if arg == "--smoke" { smokeRun = true }
        }
    }

    @MainActor static func apply(_ args: [String], to store: DocumentStore) {
        for arg in args.dropFirst() {
            if arg == "--no-scan" {
                noScan = true
            } else if arg == "--smoke" {
                smoke = true
                smokeRun = true
                store.enqueue(store.newDemo())
            } else if arg == "--demo" {
                store.enqueue(store.newDemo())
            } else if arg.hasSuffix(".vizdaw") {
                store.enqueue(store.open(path: arg))
            }
        }
    }
}

/// The `--smoke` run: Play, open the mixer, poll for 3 s through the
/// window's model, exit.
enum SmokeRun {
    @MainActor static func start(_ model: DocumentModel, openMixer: @escaping () -> Void) {
        guard LaunchArguments.smoke else { return }
        LaunchArguments.smoke = false
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            model.playStop()
            openMixer()
            var ticks: [Int64] = []
            for _ in 0..<12 {
                try? await Task.sleep(for: .milliseconds(250))
                model.poll()
                ticks.append(model.transport.tick)
                FileHandle.standardError.write(Data(
                    "smoke: \(model.selectedName ?? "-") \(Formatting.position(model.transport)) playing=\(model.isPlaying) device=\(Formatting.device(model.transport))\n".utf8))
            }
            // The mixer window polls its levels: every strip has a meter.
            let mixer = appStore.documents.first { $0.value === model }.flatMap { appStore.mixer($0.key) }
            let strips = mixer?.snapshot.strips.count ?? 0
            let meters = mixer?.meters.count ?? 0
            FileHandle.standardError.write(Data("smoke: mixer strips \(strips) meters \(meters)\n".utf8))
            let editor = await smokePluginWindow(model)
            FileHandle.standardError.write(Data("smoke: plugin window \(editor ?? "none")\n".utf8))
            let ok = model.alert == nil && (ticks.last ?? 0) > (ticks.first ?? 0)
                && strips >= 2 && meters == strips && editor != nil
            if let a = model.alert {
                FileHandle.standardError.write(Data("smoke: alert \(a.title): \(a.message)\n".utf8))
            }
            FileHandle.standardError.write(Data("smoke: \(ok ? "ok" : "FAILED")\n".utf8))
            exit(ok ? 0 : 1)
        }
    }

    /// Puts Apple's AUDelay on the first strip and opens its window: the
    /// size of the editor it shows, or nil.
    @MainActor static func smokePluginWindow(_ model: DocumentModel) async -> String? {
        guard let id = appStore.documents.first(where: { $0.value === model })?.key,
            let mixer = appStore.mixer(id), let strip = mixer.strips.first
        else { return nil }
        mixer.browser = .insert(strip: strip.id)
        mixer.pick(PluginRefRow(format: .au, id: "aufx/dely/appl", name: "AUDelay", vendor: "Apple", version: ""))
        guard let slot = mixer.snapshot.strips.first(where: { $0.id == strip.id })?.inserts.last?.id else {
            return nil
        }
        for _ in 0..<40 {
            try? await Task.sleep(for: .milliseconds(50))
            if model.slotsVersion > 0 { break }
        }
        PluginWindows.show(appStore, doc: id, slot: slot)
        try? await Task.sleep(for: .milliseconds(300))
        return PluginWindows.editorSize(doc: id, slot: slot).map { "\($0.width)x\($0.height)" }
    }
}

/// The recovery questions as modal alerts (spec §10).
@MainActor
final class RecoveryAlerts: RecoveryQuestions {
    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        let alert = NSAlert()
        alert.alertStyle = .warning
        let names = loading.map(\.name).joined(separator: ", ")
        if crashed {
            alert.messageText = loading.isEmpty
                ? "VizDAW quit unexpectedly while this project was open."
                : "VizDAW quit unexpectedly while this project was open, while loading \(names)."
        } else {
            alert.messageText = "Open “\(project)” with plugins off?"
        }
        alert.informativeText = "With plugins off, every plugin is a placeholder you can load one by one."
        alert.addButton(withTitle: "Open with Plugins Off")
        alert.addButton(withTitle: "Open Normally")
        alert.addButton(withTitle: "Cancel")
        if crashed && !loading.isEmpty {
            alert.addButton(withTitle: "Disable \(names)")
        }
        switch Self.run(alert) {
        case .alertFirstButtonReturn: return .pluginsOff
        case .alertSecondButtonReturn: return .normally
        case .alertThirdButtonReturn: return .cancel
        case NSApplication.ModalResponse(rawValue: NSApplication.ModalResponse.alertThirdButtonReturn.rawValue + 1):
            return .disable
        default: return .cancel
        }
    }

    func restoreAutosave(project: String) -> Bool {
        let alert = NSAlert()
        alert.messageText = "“\(project)” has an autosave newer than its last save."
        alert.informativeText = "Restoring opens the autosaved version as an unsaved change."
        alert.addButton(withTitle: "Restore Autosave")
        alert.addButton(withTitle: "Open Last Saved")
        return Self.run(alert) == .alertFirstButtonReturn
    }

    /// Runs `alert` in front. A launch-time open (a path argument) asks
    /// from `App.init`, before `applicationDidFinishLaunching` has made the
    /// app a regular, active app (under `swift run` there is no bundle):
    /// without this the alert can come up behind other apps' windows.
    private static func run(_ alert: NSAlert) -> NSApplication.ModalResponse {
        let app = NSApplication.shared
        if app.activationPolicy() != .regular { app.setActivationPolicy(.regular) }
        app.activate()
        return alert.runModal()
    }
}
