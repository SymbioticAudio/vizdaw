import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The one store of open documents (main actor).
@MainActor let appStore = DocumentStore.live()

/// The app's plugin catalog (main actor).
@MainActor let appPlugins = AppPlugins(open: { try PluginCatalog.open(config: $0) })

@main
struct VizDAWApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        LaunchArguments.apply(CommandLine.arguments, to: appStore)
    }

    var body: some Scene {
        WindowGroup(for: UUID.self) { $id in
            DocumentWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 980, height: 620)
        .commands { AppCommands(store: appStore) }
    }
}

/// Brings the app to the front under `swift run` (no bundle) and opens
/// packages handed over by Finder.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
        StoreAlerts.watch(appStore)
        AppSetup.startPlugins()
        PluginTicker.start(appStore)
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
    static func startPlugins() {
        let dir = supportDir
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? setSupportDir(path: dir.path)
        appPlugins.start(
            config: AppPlugins.config(supportDir: dir, scanner: scanner),
            scan: !LaunchArguments.smokeRun && !LaunchArguments.noScan)
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

/// The `--smoke` run: Play, poll for 3 s through the window's model, exit.
enum SmokeRun {
    @MainActor static func start(_ model: DocumentModel) {
        guard LaunchArguments.smoke else { return }
        LaunchArguments.smoke = false
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            model.playStop()
            var ticks: [Int64] = []
            for _ in 0..<12 {
                try? await Task.sleep(for: .milliseconds(250))
                model.poll()
                ticks.append(model.transport.tick)
                FileHandle.standardError.write(Data(
                    "smoke: \(model.selectedName ?? "-") \(Formatting.position(model.transport)) playing=\(model.isPlaying) device=\(Formatting.device(model.transport))\n".utf8))
            }
            let ok = model.alert == nil && (ticks.last ?? 0) > (ticks.first ?? 0)
            if let a = model.alert {
                FileHandle.standardError.write(Data("smoke: alert \(a.title): \(a.message)\n".utf8))
            }
            FileHandle.standardError.write(Data("smoke: \(ok ? "ok" : "FAILED")\n".utf8))
            exit(ok ? 0 : 1)
        }
    }
}
