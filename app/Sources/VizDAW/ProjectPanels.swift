import AppKit
import UniformTypeIdentifiers
import VizDAWCore

/// The New Project and Project Settings panels (spec §4.1): a modal alert
/// with a rate and a depth pop-up.
@MainActor
enum FormatPanel {
    static let depths: [(BitDepthRow, String)] = [(.int16, "16-bit"), (.int24, "24-bit"), (.float32, "32-bit float")]

    /// Asks for a format; nil when cancelled.
    static func run(title: String, message: String, initial: ProjectFormatRow, ok: String) -> ProjectFormatRow? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        let rates = NSPopUpButton()
        for r in projectRates() {
            rates.addItem(withTitle: ProjectBanners.khz(Double(r)))
            rates.lastItem?.tag = Int(r)
        }
        rates.selectItem(withTag: Int(initial.sampleRate))
        let depth = NSPopUpButton()
        for (k, (d, name)) in depths.enumerated() {
            depth.addItem(withTitle: name)
            depth.lastItem?.tag = k
            if d == initial.depth { depth.selectItem(at: k) }
        }
        let row = NSStackView(views: [NSTextField(labelWithString: "Sample rate:"), rates,
                                      NSTextField(labelWithString: "Bit depth:"), depth])
        row.orientation = .horizontal
        row.frame = NSRect(x: 0, y: 0, width: 440, height: 28)
        alert.accessoryView = row
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let d = depths.indices.contains(depth.indexOfSelectedItem) ? depths[depth.indexOfSelectedItem].0 : initial.depth
        return ProjectFormatRow(sampleRate: UInt32(rates.selectedTag()), depth: d)
    }
}

/// A modal question with one text field (Rename Media, Normalize).
@MainActor
enum TextPanel {
    /// The text entered; nil when cancelled.
    static func run(title: String, message: String, initial: String, ok: String, label: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        field.setAccessibilityLabel(label)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue
    }
}

/// A modal yes/no question (deleting media in use).
@MainActor
enum ConfirmPanel {
    static func run(title: String, message: String, ok: String) -> Bool {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
}

extension Panels {
    /// The file to relink a media item to (Locate…).
    static func chooseRelink(name: String) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.audio]
        panel.prompt = "Relink"
        panel.message = "Choose the audio file for “\(name)”."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Where to write a bounced mix; the panel itself confirms a replace
    /// (plan 5f-2: the old file stays until the new one is complete).
    static func chooseBounceFile(name: String, ext: String) -> String? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(name).\(ext)"
        if let t = UTType(filenameExtension: ext) { panel.allowedContentTypes = [t] }
        panel.canCreateDirectories = true
        panel.prompt = "Bounce"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// The folder for stems (owner answer 2: files already there are kept).
    static func chooseBounceFolder() -> String? {
        chooseFolder(prompt: "Bounce", message: "Choose a folder for the stems.")
    }

    /// A folder to search for missing media (Search Folder…).
    static func chooseFolder() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Search"
        panel.message = "Choose a folder to search for the missing audio files."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Audio files to import (any the system reads).
    static func chooseAudio() -> [String] {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.audio]
        panel.prompt = "Import"
        panel.message = "Choose audio files: they are converted into the project's format."
        return panel.runModal() == .OK ? panel.urls.map(\.path) : []
    }

    /// Where to write a MIDI export (`ext` "mid" or "midi2"); the panel
    /// itself confirms a replace (the old file stays until the new one is
    /// complete).
    static func chooseMidiExportFile(name: String, ext: String) -> String? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(name).\(ext)"
        if let t = UTType(filenameExtension: ext) { panel.allowedContentTypes = [t] }
        panel.canCreateDirectories = true
        panel.prompt = "Export"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// The folder for one MIDI Clip File per track.
    static func chooseMidiExportFolder() -> String? {
        chooseFolder(prompt: "Export", message: "Choose a folder: each track becomes one MIDI Clip File in it.")
    }

    /// The folder the export folder for another DAW goes in (plan 6b; the
    /// sheet comes first, Ruling 11).
    static func chooseExportFolder() -> String? {
        chooseFolder(prompt: "Export Here", message: "Choose where to put the folder for another DAW.")
    }

    /// The DAWproject types: the bundle's imported `org.vizdaw.dawproject`
    /// and the type the system gives the extension, which is another app's
    /// when one exports it (or a dynamic type under `swift run`). Both, as
    /// `midiTypes` lists both, so a file typed either way can be chosen.
    static var dawprojectTypes: [UTType] {
        var out: [UTType] = []
        for t in [UTType("org.vizdaw.dawproject"), UTType(filenameExtension: "dawproject")].compactMap({ $0 })
        where !out.contains(t) {
            out.append(t)
        }
        return out
    }

    /// File › Import Project › DAWproject…: the file to open as a new
    /// project (plan 6d).
    static func chooseDawprojectToOpen() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = dawprojectTypes
        panel.prompt = "Import"
        panel.message = "Choose a DAWproject file: it opens as a new project."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// The return prompt's Merge With Existing Project… when no open
    /// project matches (plan 6e Ruling 13): the `.vizdaw` package to merge
    /// into, chosen as `Panels.chooseProject()` chooses one.
    static func chooseProjectToMerge() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.treatsFilePackagesAsDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType("org.vizdaw.project"), UTType.folder].compactMap { $0 }
        panel.prompt = "Merge"
        panel.message = "Choose the VizDAW project this DAWproject came from."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Where the DAWproject export writes; `name` is the core's
    /// (`dawprojectName`). The panel itself confirms a replace, so a chosen
    /// path is sent with `replace` (the old file stays until the new one is
    /// complete).
    static func chooseDawprojectFile(name: String) -> String? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = name
        panel.allowedContentTypes = dawprojectTypes
        panel.canCreateDirectories = true
        panel.prompt = "Export"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// A folder to write into, one that can be made in the panel (F16).
    private static func chooseFolder(prompt: String, message: String) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = prompt
        panel.message = message
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Where the report sheet's Save as Text… writes.
    static func chooseReportText(name: String) -> String? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(name).txt"
        panel.allowedContentTypes = [.plainText]
        panel.canCreateDirectories = true
        panel.prompt = "Save"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}
