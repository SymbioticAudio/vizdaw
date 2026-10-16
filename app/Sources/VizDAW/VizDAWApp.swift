import AppKit
import SwiftUI
import VizDAWAppModel

/// The one store of open documents (main actor).
@MainActor let appStore = DocumentStore.live()

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
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            appStore.enqueue(appStore.open(path: url.path))
        }
    }
}

/// `VizDAW [--demo] [--smoke] [path.vizdaw ...]`: documents for the first
/// windows. `--smoke` (implies `--demo`) plays the first window for 3 s,
/// prints the transport and exits 0 if the position advanced, else 1.
enum LaunchArguments {
    @MainActor static var smoke = false

    @MainActor static func apply(_ args: [String], to store: DocumentStore) {
        for arg in args.dropFirst() {
            if arg == "--smoke" {
                smoke = true
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
