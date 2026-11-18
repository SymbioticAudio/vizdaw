import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// Categories from the window and tool in use (plan 8f Ruling 3).
@Suite struct MetricsCategoryTests {
    /// The app active, input a second ago, nothing playing, in `window`.
    func context(_ window: WindowKindRow?, detail: DetailEditorKind? = nil, tool: Tool = .arrow) -> MetricsContext {
        MetricsContext(
            recording: false, playing: false, appActive: true, secondsSinceInput: 1, window: window,
            detail: detail, tool: tool, stepEntry: false)
    }

    @Test func everyRuleInOrder() {
        // 1. Recording wins over a mixer window, and over idleness.
        var c = context(.mixer)
        c.recording = true
        #expect(MetricsCategory.classify(c) == .recording)
        c.appActive = false
        c.secondsSinceInput = nil
        #expect(MetricsCategory.classify(c) == .recording)

        // 2. Idle: not playing, and no input for more than 90 s.
        c = context(.document, detail: .pianoRoll, tool: .pencil)
        c.secondsSinceInput = 91
        #expect(MetricsCategory.classify(c) == nil)
        c.secondsSinceInput = 90
        #expect(MetricsCategory.classify(c) == .composing)
        c.secondsSinceInput = nil
        #expect(MetricsCategory.classify(c) == nil)
        // The app inactive and not playing is idle, whatever the input.
        c = context(.mixer)
        c.appActive = false
        #expect(MetricsCategory.classify(c) == nil)
        // Playing with the app inactive still counts.
        c.playing = true
        c.secondsSinceInput = nil
        #expect(MetricsCategory.classify(c) == .mixing)

        // 3. The mixer and plug-ins are mixing.
        #expect(MetricsCategory.classify(context(.mixer)) == .mixing)
        #expect(MetricsCategory.classify(context(.plugin)) == .mixing)

        // 4. Documents and editors: composing with the pencil or step
        // entry in a piano roll or drum grid; else editing.
        #expect(MetricsCategory.classify(context(.editor, detail: .pianoRoll, tool: .pencil)) == .composing)
        #expect(MetricsCategory.classify(context(.editor, detail: .pianoRoll, tool: .arrow)) == .editing)
        #expect(MetricsCategory.classify(context(.document, detail: .pianoRoll, tool: .eraser)) == .editing)
        var drum = context(.document, detail: .drumGrid, tool: .arrow)
        drum.stepEntry = true
        #expect(MetricsCategory.classify(drum) == .composing)
        #expect(MetricsCategory.classify(context(.document, detail: .list, tool: .pencil)) == .editing)
        #expect(MetricsCategory.classify(context(.document, detail: .stripChart, tool: .pencil)) == .editing)
        #expect(MetricsCategory.classify(context(.document, detail: nil, tool: .pencil)) == .editing)

        // 5. The media pool and the modify panel are editing.
        #expect(MetricsCategory.classify(context(.mediaPool)) == .editing)
        #expect(MetricsCategory.classify(context(.modifyPanel)) == .editing)

        // 6. Everything else is other.
        #expect(MetricsCategory.classify(context(.settings)) == .other)
        #expect(MetricsCategory.classify(context(.pluginManager)) == .other)
        #expect(MetricsCategory.classify(context(.other)) == .other)
        #expect(MetricsCategory.classify(context(nil)) == .other)
        #expect(MetricsCategory.idleSeconds == 90)
    }
}
