import Foundation
import VizDAWCore

/// What a time sample sees (plan 8f Ruling 3): only whether anything
/// records or plays, how long ago the last input was, and which window,
/// editor and tool are in use. Nothing of the song itself.
public struct MetricsContext: Equatable, Sendable {
    /// Any open document is recording.
    public var recording: Bool
    /// The window's document is playing.
    public var playing: Bool
    /// VizDAW is the active app.
    public var appActive: Bool
    /// Seconds since the last keyDown, mouseDown or scrollWheel; nil when
    /// there has been none.
    public var secondsSinceInput: Double?
    /// The key window's tag while the app is active, else the last one's.
    public var window: WindowKindRow?
    /// The document's detail editor, or a torn-off editor window's own.
    public var detail: DetailEditorKind?
    public var tool: Tool
    /// Step entry is on.
    public var stepEntry: Bool

    public init(
        recording: Bool, playing: Bool, appActive: Bool, secondsSinceInput: Double?, window: WindowKindRow?,
        detail: DetailEditorKind?, tool: Tool, stepEntry: Bool
    ) {
        self.recording = recording
        self.playing = playing
        self.appActive = appActive
        self.secondsSinceInput = secondsSinceInput
        self.window = window
        self.detail = detail
        self.tool = tool
        self.stepEntry = stepEntry
    }
}

/// The category a sample counts under (plan 8f Ruling 3).
public enum MetricsCategory {
    /// Input older than this, with nothing playing, is idle.
    public static let idleSeconds: Double = 90

    /// Ruling 3's rules in order; nil is idle, which is not counted.
    public static func classify(_ c: MetricsContext) -> MetricsCategoryRow? {
        if c.recording { return .recording }
        let recentInput = c.appActive && (c.secondsSinceInput.map { $0 <= idleSeconds } ?? false)
        guard c.playing || recentInput else { return nil }
        switch c.window {
        case .mixer, .plugin:
            return .mixing
        case .document, .editor:
            let notes = c.detail == .pianoRoll || c.detail == .drumGrid
            return notes && (c.tool == .pencil || c.stepEntry) ? .composing : .editing
        case .mediaPool, .modifyPanel:
            return .editing
        case .settings, .pluginManager, .other, nil:
            return .other
        }
    }
}
