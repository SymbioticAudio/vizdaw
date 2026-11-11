import AppKit
import Observation
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// Lane headers beside the arrangement (Plan Ruling 24): name (double-click
/// renames), the kind icon, monitor/mute/solo/arm, destination and input menus,
/// the input meter (plan 5d-2 Task 5), and an "Add Track" row that opens a
/// menu of the three kinds.
@MainActor
public final class LaneHeaderView: NSView, NSTextFieldDelegate, NSViewToolTipOwner, ThemedView {
    /// The theme it draws in (plan 7b Ruling 12): its host, or the themed
    /// view above it, sets it; a change redraws it and the themed views below.
    public var theme: Theme = ThemedViews.current {
        didSet { themeDidChange(from: oldValue) }
    }

    public let document: DocumentModel
    /// The preferences whose "Show channel colour bars" shows the lane's
    /// bar (controller ruling on review M3; region tints always show).
    public var appearancePrefs: AppearancePreferences? = AppearancePreferences.current {
        didSet { needsDisplay = true }
    }
    var editor: EditorModel { document.editor }
    public var verticalOffset: CGFloat = 0 {
        didSet {
            guard verticalOffset != oldValue else { return }
            needsDisplay = true
            needsLayout = true
        }
    }
    /// The lane being renamed, fixed when editing began (the shown
    /// sequence may change before it ends).
    private(set) var renaming: (seq: UInt64, lane: UInt64, name: String, field: NSTextField)?
    /// VoiceOver children, kept so their identity is stable between queries.
    private var a11yElements: [CanvasElement] = []

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        clipsToBounds = true
        wantsLayer = true
        observe()
        observeMeters()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    /// The monitor button's tooltip: `LaneInputs.help` for its mode.
    func toolTipText(at p: CGPoint) -> String? {
        guard let i = row(at: p.y), document.lanes.indices.contains(i),
            LaneHeaderLayout.control(at: p, rowTop: rowTop(i), theme: theme) == .monitor
        else { return nil }
        return LaneInputs.help(document.lanes[i].monitor)
    }

    public func view(_ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData: UnsafeMutableRawPointer?)
        -> String
    {
        toolTipText(at: point) ?? ""
    }

    /// The installed tooltip rects (one per monitor button in view) and
    /// how often they were rebuilt (tests).
    private(set) var toolTipRects: [CGRect] = []
    private(set) var toolTipRebuilds = 0

    /// One tooltip rect per monitor button in view (final review m9).
    /// AppKit's rects are in this view's coordinates and the rows scroll by
    /// `verticalOffset`, so a scroll moves them: only the rows in view get
    /// one (a screenful, however many lanes), and rects that did not change
    /// are not rebuilt (a layout for anything else, the same offset). The
    /// text is read when shown, so a monitor mode change needs no rebuild.
    private func rebuildToolTips() {
        let rects = visibleMonitorRects()
        guard rects != toolTipRects else { return }
        removeAllToolTips()
        toolTipRebuilds += 1
        toolTipRects = rects
        for r in rects { addToolTip(r, owner: self, userData: nil) }
    }

    private func visibleMonitorRects() -> [CGRect] {
        let n = document.lanes.count
        guard n > 0, laneHeight > 0, laneHeight.isFinite, bounds.height > 0 else { return [] }
        let first = ((verticalOffset - EditorCanvas.rulerHeight) / laneHeight).rounded(.down)
        let last = ((verticalOffset - EditorCanvas.rulerHeight + bounds.height) / laneHeight).rounded(.up)
        guard first.isFinite, last.isFinite else { return [] }
        let lo = Int(max(0, min(first, CGFloat(n))))
        let hi = Int(max(0, min(last, CGFloat(n - 1))))
        guard lo <= hi else { return [] }
        return (lo...hi).compactMap { i in
            LaneHeaderLayout.controls(rowTop: rowTop(i), theme: theme).first { $0.0 == .monitor }?.1
        }.filter { $0.maxY > 0 && $0.minY < bounds.height }
    }

    public override func layout() {
        super.layout()
        rebuildToolTips()
    }

    var laneHeight: CGFloat { CGFloat(editor.layout.laneHeight) }

    private func observe() {
        // What the tooltip rects depend on (rows, lane heights): redraw
        // and relayout.
        withObservationTracking {
            _ = document.lanes
            _ = editor.layout
            // The selected tracks' highlight.
            _ = editor.selection
            // The strip colours the bars take (owner G1 Q2), the mixer's
            // arrival, and the colour bars' preference.
            _ = document.mixerVersion
            _ = document.storeMixer?.snapshot
            _ = document.storeMixer?.destinations
            _ = appearancePrefs?.colourBars
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.needsLayout = true
                self?.observe()
            }
        }
    }

    /// The meters and the monitoring state only redraw: no tooltip
    /// rebuild at the poll rate (5e-2 Task 7 review M1).
    private func observeMeters() {
        withObservationTracking {
            _ = document.inputs.levels
            _ = document.inputs.clipped
            _ = document.inputs.monitoringOn
            _ = document.inputs.opening
        } onChange: { [weak self] in
            Task { @MainActor in
                if let self {
                    for r in self.meterInvalidation { self.setNeedsDisplay(r) }
                }
                self?.observeMeters()
            }
        }
    }

    /// What a meter change redraws: the union of each visible row's meter
    /// and clip rect (nothing when the lanes are too short for a meter).
    var meterInvalidation: [CGRect] {
        let n = document.lanes.count
        guard LaneHeaderLayout.showsMeter(laneHeight: laneHeight), n > 0, laneHeight.isFinite, bounds.height > 0
        else { return [] }
        return (0..<n).compactMap { i in
            let top = rowTop(i)
            guard top + laneHeight >= 0, top <= bounds.height else { return nil }
            return LaneHeaderLayout.meterRect(rowTop: top, laneHeight: laneHeight)
                .union(LaneHeaderLayout.clipRect(rowTop: top, laneHeight: laneHeight))
        }
    }

    /// The selected sequence's selected tracks.
    var selectedLanes: Set<UInt64> {
        guard let seq = document.selectedSeq else { return [] }
        return Set(editor.selection.lanes.filter { $0.seq == seq }.map(\.lane))
    }

    /// Top of row `i` in this view (below a ruler-high spacer).
    func rowTop(_ i: Int) -> CGFloat {
        EditorCanvas.rulerHeight + CGFloat(i) * laneHeight - verticalOffset
    }

    public override func draw(_ dirty: NSRect) {
        theme.rulerBackground.setFill()
        dirty.fill()
        let small = theme.font(theme.type.label)
        let inputs = document.inputs
        let selected = selectedLanes
        for (i, lane) in document.lanes.enumerated() {
            let top = rowTop(i)
            guard top + laneHeight >= dirty.minY, top <= dirty.maxY else { continue }
            if selected.contains(lane.id) {
                theme.selectionFill.withAlphaComponent(0.22).setFill()
                NSRect(x: 0, y: top, width: bounds.width, height: laneHeight).fill()
            }
            for (control, r) in LaneHeaderLayout.controls(rowTop: top, theme: theme) {
                switch control {
                case .kind: kindIcon(AppIcon.track(lane.kind), in: r)
                case .name:
                    NSString(string: lane.name).draw(
                        in: r,
                        withAttributes: [
                            .font: theme.font(theme.type.body, weight: .medium), .foregroundColor: theme.label,
                        ])
                case .monitor:
                    toggle(
                        LaneInputs.label(lane.monitor), on: lane.monitor != .off,
                        color: StripPalette.monitor(lane.monitor, theme).nsColor, in: r)
                case .mute: toggle("M", on: lane.mute, color: theme.muteColour, in: r)
                case .solo: toggle("S", on: lane.solo, color: theme.soloColour, in: r)
                case .arm: toggle("R", on: lane.arm, color: theme.armColour, in: r)
                // One line, truncated (Task 5 review M4): the input keeps
                // its channel at the end ("▾ ⇠ Ju…o · In 3").
                // An audio track shows its output path, with no menu yet.
                case .destination where lane.kind == .audio:
                    NSString(string: "→ " + (lane.audioOutName ?? "No Output")).draw(
                        in: r, withAttributes: oneLine(small, .byTruncatingTail))
                case .destination:
                    NSString(string: "▾ " + (lane.destination ?? "No Destination")).draw(
                        in: r, withAttributes: oneLine(small, .byTruncatingTail))
                case .input:
                    let badge = inputs.badge(lane).map { $0 + " · " } ?? ""
                    NSString(string: "▾ " + badge + LaneInputs.title(lane: lane)).draw(
                        in: r, withAttributes: oneLine(small, .byTruncatingMiddle))
                }
            }
            if let bar = colourBar(lane) {
                bar.setFill()
                LaneHeaderLayout.colourBar(rowTop: top, laneHeight: laneHeight).fill()
            }
            if LaneHeaderLayout.showsMeter(laneHeight: laneHeight), inputs.showsMeter(lane) {
                meter(lane.id, rowTop: top, inputs: inputs)
            }
            theme.separatorLine.setFill()
            NSRect(x: 0, y: top + laneHeight - 0.5, width: bounds.width, height: 1).fill()
        }
        let add = rowTop(document.lanes.count)
        NSString(string: "+ Add Track").draw(
            at: CGPoint(x: 8, y: add + 6),
            withAttributes: [.font: small, .foregroundColor: theme.secondaryLabel])
        theme.rulerBackground.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: EditorCanvas.rulerHeight).fill()
        theme.separatorLine.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    /// `icon` in the label colour: a template image, so it is filled with
    /// the colour where it has ink.
    private func kindIcon(_ icon: AppIcon, in r: CGRect) {
        let tinted = kindImage(icon, side: r.width)
        tinted.draw(in: r, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
    }

    /// The tinted kind images, kept per theme, icon and size.
    private var kindImages: [String: NSImage] = [:]

    private func kindImage(_ icon: AppIcon, side: CGFloat) -> NSImage {
        let key = "\(theme.id)/\(icon)/\(side)"
        if let cached = kindImages[key] { return cached }
        let image = icon.nsImage(accessibilityDescription: nil, size: side, theme: theme)
        let colour = theme.label
        let tinted = NSImage(size: CGSize(width: side, height: side), flipped: false) { rect in
            image.draw(in: rect)
            colour.set()
            rect.fill(using: .sourceIn)
            return true
        }
        if kindImages.count > 64 { kindImages = [:] }
        kindImages[key] = tinted
        return tinted
    }

    /// Secondary text on one line, cut by `mode`.
    func oneLine(_ font: NSFont, _ mode: NSLineBreakMode) -> [NSAttributedString.Key: Any] {
        let p = NSMutableParagraphStyle()
        p.lineBreakMode = mode
        return [.font: font, .foregroundColor: theme.secondaryLabel, .paragraphStyle: p]
    }

    /// The thin input meter and its clip mark (spec §6.9); the track is
    /// the theme's mid meter colour while the input is still opening
    /// (pre-flight I5); the meter colours are the theme's `meter` group.
    private func meter(_ lane: UInt64, rowTop top: CGFloat, inputs: LaneInputs) {
        let r = LaneHeaderLayout.meterRect(rowTop: top, laneHeight: laneHeight)
        let m = theme.meter
        (inputs.opening ? m.mid.nsColor.withAlphaComponent(0.5) : m.background.nsColor).setFill()
        r.fill()
        let fill = CGFloat(MixerMath.meterFill(inputs.level(lane)))
        m.low.nsColor.setFill()
        CGRect(x: r.minX, y: r.minY, width: r.width * fill, height: r.height).fill()
        (inputs.isClipped(lane) ? m.clip.nsColor : m.background.nsColor).setFill()
        LaneHeaderLayout.clipRect(rowTop: top, laneHeight: laneHeight).fill()
    }

    private func toggle(_ letter: String, on: Bool, color: NSColor, in r: CGRect) {
        let box = NSBezierPath(roundedRect: r, xRadius: theme.radius.s, yRadius: theme.radius.s)
        let fill = on ? color : theme.surface.raised.nsColor
        fill.setFill()
        box.fill()
        let s = NSString(string: letter)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: theme.font(theme.type.caption, weight: .bold),
            .foregroundColor: on ? theme.legend(on: fill) : theme.secondaryLabel,
        ]
        let size = s.size(withAttributes: attrs)
        s.draw(at: CGPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: attrs)
    }

    private func row(at y: CGFloat) -> Int? {
        guard laneHeight > 0, laneHeight.isFinite else { return nil }
        let i = Int(((y - EditorCanvas.rulerHeight + verticalOffset) / laneHeight).rounded(.down))
        return i >= 0 ? i : nil
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y) else { return }
        let lanes = document.lanes
        guard i < lanes.count else {
            if i == lanes.count { addTrackClicked(e, at: p) }
            return
        }
        let lane = lanes[i]
        let control = LaneHeaderLayout.control(at: p, rowTop: rowTop(i), theme: theme)
        // A click on the name, the icon, empty space or a menu selects the
        // track (⇧ or ⌘ adds or removes it); the toggles leave the selection.
        switch control {
        case .name, .kind, .input, .destination, nil:
            let extend = !e.modifierFlags.intersection([.shift, .command]).isEmpty
            if extend || !(e.clickCount >= 2 && control == .name) {
                editor.selectTrack(seq: seq, lane: lane.id, extend: extend)
            }
        case .monitor, .mute, .solo, .arm: break
        }
        switch control {
        case .monitor: document.inputs.cycleMonitor(lane)
        case .input: inputMenu(seq: seq, lane: lane).popUp(positioning: nil, at: p, in: self)
        case .mute: flags(seq, lane.id, mute: !lane.mute)
        case .solo: flags(seq, lane.id, solo: !lane.solo)
        case .arm: flags(seq, lane.id, arm: !lane.arm)
        // No output picker yet: a click on an audio track's output does
        // nothing (it must not set a MIDI destination on an audio track).
        case .destination where lane.kind == .audio: break
        case .destination: destinationMenu(seq: seq, lane: lane, at: p)
        case .name where e.clickCount >= 2: beginRename(lane, seq: seq, row: i)
        case .name, .kind, nil: break
        }
    }

    /// A click on the "+ Add Track" row: the menu of the three kinds. The
    /// control channel's click opens none (a menu tracks modally); the
    /// status line names the commands to use instead (Ruling 5).
    func addTrackClicked(_ e: NSEvent, at p: CGPoint) {
        guard !ControlUI.isChannel(e) else {
            editor.status = TrackCommands.addTrackOnChannel
            return
        }
        addTrackMenu().popUp(positioning: nil, at: p, in: self)
    }

    /// New Audio, Instrument and MIDI Track, each as the Track menu runs it.
    func addTrackMenu() -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let doc = document
        for (title, id) in [
            ("New Audio Track", TrackCommands.newAudio), ("New Instrument Track", TrackCommands.newInstrument),
            ("New MIDI Track", TrackCommands.newMidi),
        ] {
            menu.addItem(MenuAction(title: title) { TrackCommands.performFromMenu(id, doc) }.item(checked: false))
        }
        return menu
    }

    /// Right-click or Control-click on a lane: its menu.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y), document.lanes.indices.contains(i) else {
            return nil
        }
        return laneMenu(seq: seq, lane: document.lanes[i])
    }

    /// Track Synth… (spec §6.7) on a
    /// lane that plays an external synth, and New MIDI Destination… (plan
    /// 5d-2 Task 8).
    func laneMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let sheets = document.synth
        let track = MenuAction(title: "Track Synth…") { sheets.beginTrackSynth(seq: seq, lane: lane) }
        let trackItem = track.item(checked: false)
        trackItem.isEnabled = sheets.synth(of: lane) != nil
        menu.addItem(trackItem)
        let add = MenuAction(title: "New MIDI Destination…") { sheets.beginNewDestination(seq: seq, lane: lane) }
        menu.addItem(add.item(checked: false))
        menu.addItem(.separator())
        menu.addItem(colourItem(lane))
        return menu
    }

    /// The lane's colour bar: its strip colour, unless colour bars are off.
    func colourBar(_ lane: LaneRow) -> NSColor? {
        guard appearancePrefs?.colourBars ?? true else { return nil }
        return document.laneColour(lane, theme)?.nsColor
    }

    /// "Colour ▸": None and the eight swatches, each in its colour, for the
    /// strip the lane plays through (the strip name's menu, the same call);
    /// disabled for a lane with no strip (owner G1 Q2).
    func colourItem(_ lane: LaneRow) -> NSMenuItem {
        let item = NSMenuItem(title: "Colour", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        sub.autoenablesItems = false
        item.submenu = sub
        guard let mixer = document.mixer,
            let strip = StripPalette.laneStrip(lane, destinations: mixer.destinations, strips: mixer.snapshot.strips)
        else {
            item.isEnabled = false
            return item
        }
        let none = MenuAction(title: StripPalette.noColourTitle) { mixer.setColour(strip, nil) }
        sub.addItem(none.item(checked: strip.colour == nil))
        sub.addItem(.separator())
        for c in StripPalette.swatches {
            let a = MenuAction(title: StripPalette.title(c)) { mixer.setColour(strip, c) }
            let i = a.item(checked: strip.colour == c)
            i.image = SwatchImage.make(StripPalette.swatch(c, theme).nsColor)
            sub.addItem(i)
        }
        return item
    }

    private func flags(_ seq: UInt64, _ lane: UInt64, mute: Bool? = nil, solo: Bool? = nil, arm: Bool? = nil) {
        editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane, mute: mute, solo: solo, arm: arm)))
    }

    private func destinationMenu(seq: UInt64, lane: LaneRow, at p: CGPoint) {
        destinationMenu(seq: seq, lane: lane).popUp(positioning: nil, at: p, in: self)
    }

    /// "No Destination" and every destination. When they cannot be read the
    /// menu says so (a disabled item) instead of looking empty; a failure
    /// other than Busy also shows in the status line (spec §5).
    func destinationMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let none = MenuAction(title: "No Destination") { [weak self] in
            self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: nil)))
        }
        menu.addItem(none.item(checked: lane.midiDest == nil))
        let dests: [DestRow]
        do {
            dests = try document.session.destinations()
        } catch {
            var busy = false
            if case .Busy? = error as? VizError { busy = true }
            if !busy { editor.report(error) }
            let item = NSMenuItem(
                title: busy ? "Destinations unavailable (busy)" : "Destinations unavailable",
                action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
            return menu
        }
        for d in dests {
            let a = MenuAction(title: d.name) { [weak self] in
                self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: d.id)))
            }
            menu.addItem(a.item(checked: lane.midiDest == d.id))
        }
        if let d = dests.first(where: { $0.id == lane.midiDest }) {
            addMpe(MpeMenus.output(d), to: menu) { [weak self] c in
                guard let self else { return }
                MpeMenus.chooseOutput(c, dest: d.id, self.document)
            }
        }
        return menu
    }

    /// The input picker (spec §6.9): "No Input", the device's channels by
    /// name for an audio lane, the MIDI sources and channels for a MIDI
    /// lane; the lane's input is checked.
    func inputMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let inputs = document.inputs
        inputs.refreshInfo()
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (i, group) in inputs.groups(for: lane).enumerated() {
            if i > 0 { menu.addItem(.separator()) }
            if !group.title.isEmpty {
                let title = NSMenuItem(title: group.title, action: nil, keyEquivalent: "")
                title.isEnabled = false
                menu.addItem(title)
            }
            for choice in group.choices {
                let a = MenuAction(title: choice.title) { inputs.choose(choice, lane: lane) }
                menu.addItem(a.item(checked: choice.input == lane.input))
            }
        }
        addMpe(MpeMenus.input(lane), to: menu) { [document] c in
            MpeMenus.chooseInput(c, seq: seq, lane: lane.id, document)
        }
        return menu
    }

    /// Appends MPE sections as titled, separated groups of checked items.
    private func addMpe(_ sections: [MpeSection], to menu: NSMenu, choose: @escaping @MainActor (MpeChoice) -> Void) {
        for section in sections {
            menu.addItem(.separator())
            let title = NSMenuItem(title: section.title, action: nil, keyEquivalent: "")
            title.isEnabled = false
            menu.addItem(title)
            for c in section.choices {
                let a = MenuAction(title: c.title) { choose(c) }
                menu.addItem(a.item(checked: c.checked))
            }
        }
    }

    func beginRename(_ lane: LaneRow, seq: UInt64, row i: Int) {
        renaming?.field.removeFromSuperview()
        guard let r = LaneHeaderLayout.controls(rowTop: rowTop(i), theme: theme).first(where: { $0.0 == .name })?.1 else { return }
        let field = NSTextField(string: lane.name)
        field.frame = r.insetBy(dx: -2, dy: -2)
        field.delegate = self
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (seq, lane.id, lane.name, field)
        editor.beginTextEditing("laneName")
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("laneName")
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        r.field.removeFromSuperview()
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        guard movement != .cancel else { return }
        if !name.isEmpty, name != r.name {
            editor.edit(.lane(edit: .rename(seq: r.seq, lane: r.lane, name: name)))
        }
    }

    /// Escape cancels a rename: the name stays and no edit is sent (the
    /// gap parked from plan 4e, closed like the drum row header's).
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        cancelRename()
        return true
    }

    /// Ends a rename without renaming.
    func cancelRename() {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("laneName")
        r.field.abortEditing()
        r.field.removeFromSuperview()
        window?.makeFirstResponder(nil)
    }

    /// A header torn down mid-rename never gets its end-editing callback;
    /// give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil, renaming != nil {
            renaming = nil
            editor.endTextEditing("laneName")
        }
        // The Track menu's Rename starts here while this header is on screen
        // (Ruling 5, I11), and has nowhere to go once it leaves.
        if window != nil {
            document.beginTrackRename = { [weak self] seq, lane in
                guard let self, self.document.selectedSeq == seq,
                    let i = self.document.lanes.firstIndex(where: { $0.id == lane })
                else { return false }
                self.beginRename(self.document.lanes[i], seq: seq, row: i)
                return self.renaming != nil
            }
            document.trackRenameOwner = self
        } else if document.trackRenameOwner === self {
            // Only its own: a second header's hook stays.
            document.beginTrackRename = nil
            document.trackRenameOwner = nil
        }
    }

    public override func scrollWheel(with e: NSEvent) {
        nextResponder?.scrollWheel(with: e)
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Track Headers" }

    public override func accessibilityChildren() -> [Any]? {
        guard let seq = document.selectedSeq else {
            a11yElements = []
            return []
        }
        let inputs = document.inputs
        let selected = selectedLanes
        let items = document.lanes.enumerated().map { i, lane in
            let r = NSRect(x: 0, y: rowTop(i), width: bounds.width, height: laneHeight)
            let opening = inputs.opening && inputs.showsMeter(lane)
            let item = CanvasAccessibilityItem(
                label: ArrangementA11y.kindName(lane.kind) + ", "
                    + ArrangementA11y.header(lane, returnOf: inputs.returnName(lane)) + (opening ? ", input opening" : ""),
                frame: r, selected: selected.contains(lane.id),
                press: { [weak self] in self?.flags(seq, lane.id, mute: !lane.mute) })
            return (item: item, frame: r)
        }
        // The "+ Add Track" row is the last child; its press opens the menu.
        let addFrame = NSRect(x: 0, y: rowTop(document.lanes.count), width: bounds.width, height: laneHeight)
        let add = CanvasAccessibilityItem(
            label: "Add Track", frame: addFrame, selected: false,
            press: { [weak self] in
                guard let self else { return }
                self.addTrackMenu().popUp(positioning: nil, at: CGPoint(x: addFrame.minX + 8, y: addFrame.midY), in: self)
            })
        // Reused like EditorCanvas's, so their identity is stable between queries.
        a11yElements = CanvasElement.reuse(a11yElements, for: items + [(item: add, frame: addFrame)], parent: self)
        return a11yElements
    }
}

/// A menu item that runs a closure.
@MainActor
final class MenuAction: NSObject {
    let title: String
    let run: @MainActor () -> Void
    private static var alive: [MenuAction] = []

    init(title: String, run: @escaping @MainActor () -> Void) {
        self.title = title
        self.run = run
    }

    func item(checked: Bool) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: #selector(fire), keyEquivalent: "")
        i.target = self
        i.state = checked ? .on : .off
        Self.alive.append(self)
        if Self.alive.count > 512 { Self.alive.removeFirst(256) }
        return i
    }

    @objc func fire() { run() }
}

/// A menu item's colour dot (the Colour menus): not a template, so the menu
/// draws it in its colour.
public enum SwatchImage {
    @MainActor public static func make(_ colour: NSColor, side: CGFloat = 12) -> NSImage {
        let image = NSImage(size: CGSize(width: side, height: side), flipped: false) { r in
            colour.setFill()
            NSBezierPath(ovalIn: r.insetBy(dx: 1, dy: 1)).fill()
            return true
        }
        image.isTemplate = false
        return image
    }
}
