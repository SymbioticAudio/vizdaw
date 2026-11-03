import AppKit
import Foundation
import UniformTypeIdentifiers
import VizDAWAppModel
import VizDAWCore

/// Watches one path with a `DispatchSource` file-system-object source for
/// write, rename or delete events (spec §11; plan 5f-3 Ruling 4), on the
/// main queue. An editor that saves by replacing the file leaves the
/// source on the old file: on a rename or delete the source is cancelled
/// and the path reopened every 0.5 s until it exists again; the new file
/// is then watched and reported as a change (Review Focus 1; the e2e's
/// second, in-place save pins it).
@MainActor
final class LiveFileWatching: FileWatching {
    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        let w = FileWatch(path: path, changed: changed)
        return w.arm() ? w : nil
    }
}

/// One path's watch; used on the main queue only. Released, it cancels
/// its source (which closes the descriptor) and stops reopening.
private final class FileWatch: @unchecked Sendable {
    let path: String
    let changed: @MainActor () -> Void
    private var source: DispatchSourceFileSystemObject?

    init(path: String, changed: @escaping @MainActor () -> Void) {
        self.path = path
        self.changed = changed
    }

    deinit { source?.cancel() }

    /// Opens the path (`O_EVTONLY`) and starts its source; false when it
    /// cannot be opened.
    func arm() -> Bool {
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else { return false }
        let s = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd, eventMask: [.write, .rename, .delete], queue: .main)
        s.setEventHandler { [weak self] in
            MainActor.assumeIsolated { self?.event() }
        }
        s.setCancelHandler { close(fd) }
        source = s
        s.resume()
        return true
    }

    @MainActor private func event() {
        guard let s = source else { return }
        let flags = s.data
        changed()
        if flags.contains(.rename) || flags.contains(.delete) {
            s.cancel()
            source = nil
            reopen()
        }
    }

    @MainActor private func reopen() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.arm() { self.changed() } else { self.reopen() }
            }
        }
    }
}

/// Opens the copy in the chosen application with `NSWorkspace` (Ruling 5).
/// The system reports a failure later, in its completion (pre-flight M9).
@MainActor
final class WorkspaceOpener: ExternalOpener {
    func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        guard let app else { throw VizError.Model(message: "Choose an external editor in Settings › Editing.") }
        guard FileManager.default.fileExists(atPath: app.path) else {
            throw VizError.Io(message: "\(app.path) is not an application.")
        }
        NSWorkspace.shared.open([file], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration()) {
            _, error in
            guard let error else { return }
            let message = error.localizedDescription
            Task { @MainActor in failed(message) }
        }
    }
}

/// The application chooser of Settings › Editing and of the first Region ›
/// Open in External Editor (Ruling 5): applications only. Never shown for
/// a key, a surface button or the channel (owner answer 2026-11-03).
@MainActor
enum ExternalEditorPanel {
    static func choose() -> String? {
        let panel = NSOpenPanel()
        panel.title = "Choose External Sample Editor"
        panel.prompt = "Choose"
        panel.message = "Choose the application that edits audio files for Open in External Editor."
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.treatsFilePackagesAsDirectories = false
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}
