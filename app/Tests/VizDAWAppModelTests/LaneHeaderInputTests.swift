import AppKit
import SymbioticTokens
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The lane header's input controls (plan 5d-2 Task 5; spec §6.9): where
/// they sit, the input menu, and what VoiceOver says.
@MainActor
@Suite struct LaneHeaderInputTests {
    /// Song's Chords lane (31) plays to an audio path; the device has two
    /// named channels.
    func header(armed: Bool = false) throws -> (DocumentModel, FakeSession, LaneHeaderView) {
        let f = editorFake()
        f.laneTable[3]?[1].audioOut = 7
        if armed {
            f.laneTable[3]?[1].arm = true
            f.laneTable[3]?[1].input = .audio(first: 0, stereo: false, returnOf: nil)
        }
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        let m = try DocumentModel(session: f)
        return (m, f, LaneHeaderView(document: m))
    }

    @Test func theInputMenuListsTheChoicesAndSetsTheInput() throws {
        let (m, f, h) = try header()
        let items = h.inputMenu(seq: 3, lane: m.lanes[1]).items.filter { !$0.isSeparatorItem }
        #expect(items.map(\.title) == ["No Input", "Audio", "In 1 (L)", "In 2 (R)", "In 1+2"])
        #expect(items[0].state == .on, "the lane has no input yet")
        #expect(!items[1].isEnabled, "a section title")
        let pair = items[4]
        _ = pair.target?.perform(pair.action)
        #expect(
            f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 0, stereo: true, returnOf: nil))))
    }

    /// A header in a window, with the track `lane` of Song selected.
    func shown(select lane: UInt64? = nil) throws -> (DocumentModel, FakeSession, LaneHeaderView, NSWindow) {
        let (m, f, h) = try header()
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled], backing: .buffered,
            defer: false)
        w.isReleasedWhenClosed = false
        h.frame = NSRect(x: 0, y: 0, width: LaneHeaderLayout.width, height: 300)
        w.contentView?.addSubview(h)
        if let lane {
            m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(3, lane).ref], events: []))
        }
        return (m, f, h, w)
    }

    /// Rename Track from the menu or a key reaches the header on screen
    /// (plan tracks Ruling 5, I11); with none showing it says where to go.
    @Test func renameTrackGoesThroughTheHeader() throws {
        let (m, f, h, w) = try shown(select: 31)
        defer { w.close() }
        #expect(m.beginTrackRename != nil, "the header installed it")
        #expect(TrackCommands.perform(TrackCommands.rename, m) == .done)
        let field = try #require(h.renaming?.field)
        #expect(field.stringValue == "Chords")
        #expect((w.firstResponder as? NSTextView)?.delegate === field, "the rename field has the keyboard")
        h.cancelRename()
        // Leaving the window clears it: nothing to rename in.
        h.removeFromSuperview()
        #expect(m.beginTrackRename == nil)
        #expect(
            TrackCommands.perform(TrackCommands.rename, m) == .unavailable("Show Arrange to rename a track."))
        // The channel names the track and sends the rename.
        let named = TrackCommands.run(
            TrackCommands.rename, args: [ControlArg(name: "name", value: .text(value: "Bass"))], doc: UUID(), m)
        if case .failed = named { Issue.record("\(named)") }
        #expect(f.edits.last == .lane(edit: .rename(seq: 3, lane: 31, name: "Bass")))
    }

    /// Ruling 6: each header names its kind first, and the kind icon sits
    /// before the name at the label's size.
    @Test func theHeaderShowsEachTracksKind() throws {
        let f = editorFake()
        f.laneTable[3] = [lane(40, "A", kind: .audio), lane(41, "B", kind: .instrument), lane(42, "C", kind: .midi)]
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let labels = (h.accessibilityChildren() ?? []).compactMap { ($0 as? NSAccessibilityElement)?.accessibilityLabel() }
        #expect(labels.count == 4)
        #expect(labels[0].hasPrefix("Audio track, A, "))
        #expect(labels[1].hasPrefix("Instrument track, B, "))
        #expect(labels[2].hasPrefix("MIDI track, C, "))
        #expect(labels[3] == "Add Track")
        for theme in [Theme.classicLight, .studioDark, .console] {
            let controls = LaneHeaderLayout.controls(rowTop: 100, theme: theme)
            let kind = try #require(controls.first { $0.0 == .kind }?.1)
            let name = try #require(controls.first { $0.0 == .name }?.1)
            let monitor = try #require(controls.first { $0.0 == .monitor }?.1)
            #expect(kind.width == theme.type.label.size && kind.height == kind.width, "\(theme.id)")
            #expect(kind.minX == 8)
            #expect(name.minX == kind.maxX + theme.spacing.xs, "\(theme.id)")
            #expect(kind.midY == name.midY)
            #expect(monitor.minX == 113 && name.maxX <= monitor.minX)
        }
    }

    /// Ruling 5: a click the channel sends opens no menu (it would track
    /// modally); the status line names the commands to use.
    @Test func aChannelClickOnAddTrackOpensNoMenu() throws {
        let (m, f, h) = try header()
        h.frame = NSRect(x: 0, y: 0, width: LaneHeaderLayout.width, height: 300)
        let at = CGPoint(x: 20, y: h.rowTop(m.lanes.count) + 10)
        let calls = f.calls.count
        let e = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: h.convert(at, to: nil), modifierFlags: [], timestamp: 1,
                windowNumber: 0, context: nil, eventNumber: ControlUI.channelEventBase + 1, clickCount: 1,
                pressure: 1))
        #expect(ControlUI.isChannel(e))
        h.mouseDown(with: e)
        #expect(
            m.editor.status
                == "Add Track opens a menu: use track.new_audio, track.new_instrument or track.new_midi over the control channel."
        )
        #expect(f.calls.count == calls && f.newTracks.isEmpty && f.edits.isEmpty)
        #expect(h.addTrackMenu().items.map(\.title) == ["New Audio Track", "New Instrument Track", "New MIDI Track"])
    }

    @Test func theControlsShareTheRowWithoutOverlapping() {
        let controls = LaneHeaderLayout.controls(rowTop: 0, theme: .classicLight)
        for (i, a) in controls.enumerated() {
            for b in controls.dropFirst(i + 1) {
                #expect(!a.1.intersects(b.1), "\(a.0) and \(b.0)")
            }
            #expect(a.1.maxX <= LaneHeaderLayout.width)
        }
        let meter = LaneHeaderLayout.meterRect(rowTop: 0, laneHeight: 48)
        #expect(controls.allSatisfy { !$0.1.intersects(meter) })
        #expect(meter.maxY <= 48, "inside the default lane height")
        #expect(LaneHeaderLayout.clipRect(rowTop: 0, laneHeight: 48).minX >= meter.maxX)
    }

    @Test func theMeterRedrawCoversOnlyTheMeters() throws {
        let f = editorFake()
        f.laneTable[3]?.append(lane(32, "Third"))
        f.layout = #"{"laneHeight":60}"#
        let m = try DocumentModel(session: f)
        m.select(3)
        let h = LaneHeaderView(document: m)
        h.frame = NSRect(x: 0, y: 0, width: LaneHeaderLayout.width, height: 400)
        let rects = h.meterInvalidation
        #expect(rects.count == 3)
        for (i, r) in rects.enumerated() {
            let band = LaneHeaderLayout.meterRect(rowTop: h.rowTop(i), laneHeight: 60)
            #expect(r.height == 3 && r.minY == band.minY && r.minX == band.minX, "row \(i)")
            #expect(r.maxX >= LaneHeaderLayout.clipRect(rowTop: h.rowTop(i), laneHeight: 60).maxX)
        }
        f.layout = #"{"laneHeight":40}"#
        let short = try DocumentModel(session: f)
        short.select(3)
        let hs = LaneHeaderView(document: short)
        hs.frame = h.frame
        #expect(hs.meterInvalidation.isEmpty)
    }

    @Test func voiceOverSaysTheInputAndMonitoring() throws {
        var l = lane(31, "Vocal")
        l.arm = true
        l.input = .audio(first: 0, stereo: true, returnOf: nil)
        l.monitor = .input
        #expect(ArrangementA11y.header(l) == "Vocal, no destination, armed, input In 1+2, monitoring input")
        l.monitor = .off
        #expect(ArrangementA11y.header(l).hasSuffix("monitoring off"))
        l.monitor = .auto
        #expect(ArrangementA11y.header(l) == "Vocal, no destination, armed, input In 1+2")
        // A return lane says whose return it records (Task 5 review M3).
        #expect(
            ArrangementA11y.header(l, returnOf: "Moog")
                == "Vocal, no destination, armed, input In 1+2, recording the return of Moog")
    }

    /// Pre-flight I5: the header says so while the input opens.
    @Test func voiceOverSaysTheInputIsOpening() throws {
        let (m, f, h) = try header(armed: true)
        f.inputRow.opening = true
        m.inputs.pollMeters()
        #expect(m.inputs.opening)
        let labels = (h.accessibilityChildren() ?? []).compactMap { ($0 as? NSAccessibilityElement)?.accessibilityLabel() }
        #expect(labels.contains { $0.hasSuffix(", input opening") })
    }

    /// Task 4 review M2: no empty meter frame before the first poll.
    @Test func monitoringStartsFromTheCoresSetting() throws {
        let saved = LaneInputs.softwareMonitoring
        defer { LaneInputs.softwareMonitoring = saved }
        LaneInputs.softwareMonitoring = { false }
        let (m, _, _) = try header()
        #expect(!m.inputs.monitoringOn)
    }

    /// The B4 run found the lane header column dark in Classic Light: it
    /// takes the theme's header background in every theme (plan 7b Task B5).
    @Test func theHeaderColumnIsInTheTheme() throws {
        let (m, _, h) = try header()
        defer { withExtendedLifetime(m) {} }
        h.frame = NSRect(x: 0, y: 0, width: 200, height: 400)
        for t in [Theme.classicLight, .studioDark, .console] {
            h.theme = t
            let rep = try #require(h.bitmapImageRepForCachingDisplay(in: h.bounds))
            h.cacheDisplay(in: h.bounds, to: rep)
            let sx = rep.pixelsWide / 200
            for (x, y) in [(100, 5), (150, 390)] {
                let p = try #require(rep.colorAt(x: x * sx, y: y * sx)?.usingColorSpace(.sRGB))
                let want = try #require(t.rulerBackground.usingColorSpace(.sRGB))
                #expect(
                    abs(p.redComponent - want.redComponent) < 0.04 && abs(p.greenComponent - want.greenComponent) < 0.04
                        && abs(p.blueComponent - want.blueComponent) < 0.04, "\(t.id) at \(x),\(y): \(p)")
            }
        }
    }
}
