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
}
