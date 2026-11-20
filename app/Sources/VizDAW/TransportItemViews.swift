import AppKit
import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The app's views of the transport items (groups-vcas-transport Task 11):
/// one `NSView` per `TransportItemID`, hosting SwiftUI where the item is
/// SwiftUI today. What each item does lives in `TransportItemEntries`
/// (pre-flight F9); the readouts, the fields, spaces and separators are
/// `TransportRowView`'s own.
@MainActor
enum TransportItemViews {
    /// `item`'s view in `docID`'s window; nil for the row's own views.
    static func make(_ item: TransportItem, model: DocumentModel, docID: UUID, documentWindow: Bool) -> NSView? {
        let host: RecordPopoverHost = documentWindow ? .document : .mixerWindow
        if case .command(let cmd, let label) = item {
            return hosted(CommandButtonItem(model: model, docID: docID, command: cmd, label: label))
        }
        switch item.id {
        case .cluster: return hosted(TransportCluster(model: model))
        case .loop: return hosted(LoopItem(model: model))
        case .click: return ClickItemView(document: model)
        case .replaceOverdub: return hosted(ReplaceOverdubButton(record: model.record))
        case .recordOptions: return hosted(RecordOptionsButton(model: model, record: model.record, host: host))
        case .countIn: return hosted(CountInItem(record: model.record))
        case .summary: return hosted(StepSummaryItem(model: model))
        case .dspLoad: return DspLoadView(load: { appStore.mixer(docID)?.dspLoad ?? 0 }, poll: { appStore.mixer(docID)?.pollLevels() })
        case .device: return hosted(DeviceItem(model: model))
        case .faults: return hosted(FaultsItem(model: model))
        case .mainView: return hosted(MainViewItem(model: model, editor: model.editor))
        case .panes: return hosted(PanesItem(model: model, editor: model.editor))
        case .bar, .time, .tempo, .signature, .positionField, .editFields, .punchFields, .space, .separator,
            .command:
            return nil
        }
    }

    /// A SwiftUI item in the app's theme, sized to its ideal size.
    static func hosted<V: View>(_ v: V) -> NSView {
        let h = TransportItemHost(rootView: v.fixedSize().modifier(ItemTheme(prefs: appearancePreferences)))
        // Only its ideal size: the row sets its frame.
        h.sizingOptions = [.intrinsicContentSize]
        return h
    }
}

/// A hosted item: when its content's size changes, the row lays out again.
final class TransportItemHost<Content: View>: NSHostingView<Content> {
    required init(rootView: Content) {
        super.init(rootView: rootView)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { nil }

    override func invalidateIntrinsicContentSize() {
        super.invalidateIntrinsicContentSize()
        superview?.needsLayout = true
    }
}

/// The theme for an item (as `ThemedRoot`, without its window background:
/// the bar's own shows through).
private struct ItemTheme: ViewModifier {
    let prefs: AppearancePreferences

    func body(content: Content) -> some View {
        let t = prefs.theme
        content
            .symbioticTheme(t)
            .foregroundStyle(t.text.primary.color, t.text.secondary.color, t.text.disabled.color)
            .tint(t.accent.accent.color)
    }
}

/// The fixed cluster (Ruling 13): Return to Start, Rewind, Fast Forward,
/// Play/Stop (one button) and Record with its readouts. AppKit buttons:
/// the channel's tree walks AppKit's elements (final review I1).
struct TransportCluster: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: DocumentModel

    var body: some View {
        HStack(spacing: theme.spacing.s) {
            AppKitButton(
                "Return to Start", icon: .returnToStart, enabled: model.selectedSeq != nil, help: "Return to Start"
            ) {
                do { try model.moveTo(0) } catch { model.editor.status = TransportCommands.statusText(error) }
            }
            .fixedSize()
            holdButton(
                "Rewind", icon: .rewind, forward: false, help: "Rewind one bar; hold to wind; ⇧-click: previous marker")
            holdButton(
                "Fast Forward", icon: .fastForward, forward: true,
                help: "Fast Forward one bar; hold to wind; ⇧-click: next marker")
            AppKitButton(
                model.isPlaying ? "Stop" : "Play", icon: model.isPlaying ? .stop : .play,
                enabled: model.canPlay, help: model.isPlaying ? "Stop (Space)" : "Play (Space)"
            ) { model.playStop() }
            .fixedSize()
            RecordClusterPart(record: model.record)
        }
    }

    private func holdButton(_ title: String, icon: AppIcon, forward: Bool, help: String) -> some View {
        let m = model
        return AppKitHoldButton(
            title, icon: icon, enabled: m.selectedSeq != nil, help: help,
            onPress: { shift in
                m.shuttle.press(forward: forward, shift: shift, at: ProcessInfo.processInfo.systemUptime)
            },
            onTick: { m.shuttle.tick(at: ProcessInfo.processInfo.systemUptime) },
            onRelease: { inside in
                m.shuttle.release(at: ProcessInfo.processInfo.systemUptime, inside: inside)
            },
            onCancel: { m.shuttle.cancel() }
        )
        .fixedSize()
    }
}

/// A command button (Task 12, Ruling 16): its label (the command's title
/// unless given) with its glyph when it has one; its tooltip and
/// accessibility label the command's title; greyed while its menu item
/// would be, or when the registry no longer has its id.
struct CommandButtonItem: View {
    let model: DocumentModel
    let docID: UUID
    let command: String
    let label: String?

    var body: some View {
        let face = CommandButtonFace.make(id: command, label: label, row: keyBindings.row(command), doc: model)
        let m = model
        let id = docID
        let cmd = command
        AppKitButton(
            face.label, icon: face.icon, enabled: face.enabled, help: face.tooltip, label: face.accessibilityLabel,
            showsTitle: true
        ) {
            CommandButtonAction.run(
                cmd, doc: (id, m), store: appStore, host: AppControlHost.shared, menu: AppMenuPerformer())
        }
    }
}

/// The app's main menu as a command button falls back to it (Ruling 16):
/// the top-level menu titled `menu`, the item titled `title` in it (or in
/// its submenus), performed when it is enabled.
@MainActor
struct AppMenuPerformer: MenuPerformer {
    func perform(menu: String, title: String) -> Bool {
        guard let top = NSApp.mainMenu?.items.first(where: { $0.title == menu })?.submenu else { return false }
        return Self.perform(title, in: top)
    }

    private static func perform(_ title: String, in menu: NSMenu) -> Bool {
        menu.update()
        for (n, item) in menu.items.enumerated() {
            if item.title == title, item.submenu == nil {
                guard item.isEnabled else { return false }
                menu.performActionForItem(at: n)
                return true
            }
            if let sub = item.submenu, perform(title, in: sub) { return true }
        }
        return false
    }
}

struct LoopItem: View {
    @Bindable var model: DocumentModel

    var body: some View {
        AppKitButton("Loop", icon: .loop, isOn: model.looping, help: "Loop") { model.toggleLoop() }
    }
}

/// Count-in (Ruling 13): a toggle of the record options' count-in.
struct CountInItem: View {
    @Bindable var record: RecordModel

    var body: some View {
        AppKitButton(
            "Count-in", isOn: record.countInBars > 0, enabled: !record.isRecording,
            help: "Count in before recording (the bars are in Record Options)"
        ) { record.toggleCountIn() }
    }
}

/// Step entry's summary, when there is one (plan 4f Ruling 11).
struct StepSummaryItem: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel

    var body: some View {
        if let s = model.stepEntry.summary {
            Text(s)
                .font(theme.type.caption.font)
                .padding(.horizontal, theme.spacing.s)
                .padding(.vertical, theme.spacing.xxs)
                .background(
                    theme.accent.accent.color.opacity(0.18), in: RoundedRectangle(cornerRadius: theme.radius.s))
                .accessibilityLabel(s)
        }
    }
}

/// The output device and its rate.
struct DeviceItem: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel

    var body: some View {
        AppKitLabel(Formatting.device(model.transport), font: theme.type.caption.nsFont, secondary: true)
    }
}

/// The faults button (document window only, so `showsFaults` opens one
/// popover: Ruling 8), shown while there are faults or a report.
struct FaultsItem: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: DocumentModel

    var body: some View {
        if model.hasFaults || model.hasReport {
            AppKitButton(
                "Faults", icon: .faultIndicator, help: "Faults and compile messages",
                tint: model.hasFaults ? theme.state.error.nsColor : theme.state.warning.nsColor
            ) { model.showsFaults.toggle() }
            .popover(isPresented: $model.showsFaults) {
                VStack(alignment: .leading, spacing: theme.spacing.xs) {
                    ForEach(Array(model.messages.enumerated()), id: \.offset) {
                        Text($0.element)
                    }
                }
                .padding(theme.spacing.l)
                .frame(minWidth: 240)
            }
        }
    }
}

/// The main-view tabs, through the View menu's commands (`ui.*`).
struct MainViewItem: View {
    let model: DocumentModel
    @Bindable var editor: EditorModel

    var body: some View {
        Picker(
            "Main View",
            selection: Binding(
                get: { editor.layout.views.main },
                set: { ViewCommands.run($0 == .arrange ? "ui.view_arrange" : "ui.view_mix", on: nil, model) })
        ) {
            ForEach(MainView.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .accessibilityLabel("Main View")
    }
}

/// The pane toggles, through the View menu's commands.
struct PanesItem: View {
    @Environment(\.symbioticTheme) private var theme
    let model: DocumentModel
    @Bindable var editor: EditorModel

    var body: some View {
        HStack(spacing: theme.spacing.s) {
            ForEach(TransportItemEntries.panes, id: \.id) { p in
                // Read here, so a pane change redraws the toggle.
                let _ = editor.layout.views
                AppKitButton(
                    p.side.title, icon: icon(p.side), isOn: ViewCommands.isOn(p.id, model),
                    help: "\(p.side.title) (\(p.key))"
                ) { _ = ViewCommands.run(p.id, on: nil, model) }
                .fixedSize()
            }
        }
    }

    private func icon(_ side: PaneSide) -> AppIcon {
        switch side {
        case .left: .leftPane
        case .bottom: .bottomPane
        case .right: .rightPane
        }
    }
}

/// Click (Ruling 13): the metronome's toggle (`transport.click`) and its
/// ⌄. A right-click on either, or the ⌄, opens Level › and Mode ›
/// (`TransportItemEntries.clickMenu`). Off and disabled while the session
/// has not given the settings.
final class ClickItemView: NSView, ThemedView {
    var theme: Theme = ThemedViews.current {
        didSet {
            guard theme != oldValue else { return }
            needsDisplay = true
            refresh()
        }
    }

    let document: DocumentModel
    let toggle = ItemMenuButton(title: "Click", target: nil, action: nil)
    let options = ItemMenuButton(title: "", target: nil, action: nil)

    init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        clipsToBounds = true
        for b in [toggle, options] {
            b.bezelStyle = .push
            b.refusesFirstResponder = true
            b.target = self
            addSubview(b)
        }
        toggle.setButtonType(.pushOnPushOff)
        toggle.action = #selector(togglePressed(_:))
        toggle.setAccessibilityLabel("Click")
        toggle.cell?.setAccessibilityLabel("Click")
        toggle.toolTip = "The metronome on or off"
        options.action = #selector(optionsPressed(_:))
        options.imagePosition = .imageOnly
        options.setAccessibilityLabel("Click Options")
        options.toolTip = "Click level and mode"
        refresh()
        observe()
    }

    required init?(coder: NSCoder) { nil }

    private func observe() {
        withObservationTracking {
            _ = document.transport.click
            refresh()
        } onChange: { [weak self] in
            DispatchQueue.main.async { self?.observe() }
        }
    }

    private func refresh() {
        let on = TransportItemEntries.clickIsOn(document)
        toggle.state = on ? .on : .off
        toggle.isEnabled = document.transport.click != nil
        toggle.contentTintColor = on ? theme.accent.accent.nsColor : nil
        options.image = AppIcon.menuDown.nsImage(accessibilityDescription: "Click Options", theme: theme)
        options.isEnabled = document.transport.click != nil
    }

    @objc private func togglePressed(_ sender: NSButton) {
        // The button shows the model, not its own flip.
        sender.state = TransportItemEntries.clickIsOn(document) ? .on : .off
        TransportItemEntries.toggleClick(document)
    }

    @objc private func optionsPressed(_ sender: NSButton) {
        guard !ControlUI.isChannel(NSApp.currentEvent) else {
            document.editor.status = Self.channelNote
            return
        }
        clickMenu().popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 2), in: sender)
    }

    static let channelNote =
        "Click's menu does not open for the control channel; use transport.set_click_level or transport.set_click_mode."

    private func clickMenu() -> NSMenu {
        TransportMenus.menu(TransportItemEntries.clickMenu(document)) { _ in }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        ControlUI.isChannel(event) ? nil : clickMenu()
    }

    override var intrinsicContentSize: NSSize {
        let a = toggle.intrinsicContentSize
        let b = options.intrinsicContentSize
        return NSSize(width: a.width + theme.spacing.xxs + b.width, height: max(a.height, b.height))
    }

    override func layout() {
        super.layout()
        let a = toggle.intrinsicContentSize
        let b = options.intrinsicContentSize
        toggle.frame = NSRect(x: 0, y: (bounds.height - a.height) / 2, width: a.width, height: a.height)
        options.frame = NSRect(
            x: a.width + theme.spacing.xxs, y: (bounds.height - b.height) / 2, width: b.width, height: b.height)
    }
}

/// A button whose right-click gives its item's menu.
final class ItemMenuButton: NSButton {
    override init(frame: NSRect) {
        super.init(frame: frame)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { nil }

    override func menu(for event: NSEvent) -> NSMenu? { superview?.menu(for: event) }
}

/// DSP load (Ruling 13): the engine's load, polled through the document's
/// mixer model at most four times a second while it shows.
final class DspLoadView: NSView, ThemedView {
    var theme: Theme = ThemedViews.current {
        didSet {
            guard theme != oldValue else { return }
            label.font = theme.digitsFont(theme.type.caption)
            label.textColor = theme.secondaryLabel
            invalidateIntrinsicContentSize()
        }
    }

    let label = NSTextField(labelWithString: TransportItemEntries.dspText(0))
    private let load: @MainActor () -> Float
    private let poll: @MainActor () -> Void
    private var timer: Timer?

    init(load: @escaping @MainActor () -> Float, poll: @escaping @MainActor () -> Void) {
        self.load = load
        self.poll = poll
        super.init(frame: .zero)
        clipsToBounds = true
        label.refusesFirstResponder = true
        label.setAccessibilityElement(false)
        label.font = theme.digitsFont(theme.type.caption)
        label.textColor = theme.secondaryLabel
        addSubview(label)
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
        setAccessibilityLabel("DSP Load")
        setAccessibilityValue(label.stringValue)
    }

    required init?(coder: NSCoder) { nil }

    /// Wide enough for "DSP 100%", so the digits never move the row.
    override var intrinsicContentSize: NSSize {
        let f = NSTextField(labelWithString: TransportItemEntries.dspText(1))
        f.font = label.font
        return f.intrinsicContentSize
    }

    override func layout() {
        super.layout()
        label.frame = bounds
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateTimer()
    }

    override func viewDidHide() {
        super.viewDidHide()
        updateTimer()
    }

    override func viewDidUnhide() {
        super.viewDidUnhide()
        updateTimer()
    }

    /// Polls only while it is in a window and shown.
    private func updateTimer() {
        let shows = window != nil && !isHiddenOrHasHiddenAncestor
        if shows, timer == nil {
            let box = DspTimerBox()
            box.view = self
            let t = Timer(timeInterval: 0.25, repeats: true) { _ in
                MainActor.assumeIsolated { box.view?.tick() }
            }
            RunLoop.main.add(t, forMode: .common)
            timer = t
        } else if !shows {
            timer?.invalidate()
            timer = nil
        }
    }

    private func tick() {
        poll()
        let text = TransportItemEntries.dspText(load())
        if label.stringValue != text { label.stringValue = text }
        setAccessibilityValue(text)
    }
}

/// Holds the DSP view for the timer's `@Sendable` block (Swift 6.1: the
/// block captures this `@MainActor` class, never the view).
@MainActor
private final class DspTimerBox {
    weak var view: DspLoadView?
}
