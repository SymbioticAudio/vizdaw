import AppKit
import SwiftUI
import os
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// "Allow Co-Producer to Change Your Song?" (plan 8b Ruling 11; spec §6.8):
/// VizDAW's own approval of a Co-Producer change that needs the composer's
/// yes, the one owner-approved sheet a Co-Producer call may open. Don't
/// Allow is the default and Esc, so Return never allows. Allow and Always
/// Allow Anyway count only for a real press (`CoProducerApproval.isRealPress`
/// of the current event against this sheet's window); a refused press
/// shows a line and logs the event's fields for the alpha's manual checks.
/// For a caller VizDAW cannot verify, Always allow shows the warning first.
/// All state lives in `CoProducerApprovalSheetModel`.
struct CoProducerApprovalSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: CoProducerApprovalSheetModel

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text(model.title)
                .font(theme.type.title.font)
                .foregroundStyle(theme.text.primary.color)
                .accessibilityAddTraits(.isHeader)
            switch model.step {
            case .ask: ask
            case .warning: warning
            }
            if let refused = model.refusedPress {
                Text(refused)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.state.warning.color)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel(refused)
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 520)
        .background(SheetWindowReader(model: model).accessibilityHidden(true))
    }

    static let log = Logger(subsystem: "VizDAW", category: "co-producer-approval")

    /// Whether the press now is the composer's own; a refusal is logged
    /// with what it saw (never accepted on that account).
    @MainActor static func realPress(_ model: CoProducerApprovalSheetModel, button: String) -> Bool {
        let p = CoProducerApproval.currentPress(window: model.window)
        let ok = CoProducerApproval.isRealPress(p)
        if !ok {
            let type = p.type.map { "\($0.rawValue)" } ?? "none"
            let key = p.keyCode.map { "\($0)" } ?? "-"
            log.notice(
                "refused \(button, privacy: .public) press: type \(type, privacy: .public) key \(key, privacy: .public) in sheet window \(p.inSheetWindow, privacy: .public) source pid \(p.sourcePid, privacy: .public) own pid \(p.ownPid, privacy: .public) age \(p.age, privacy: .public) s"
            )
        }
        return ok
    }

    private var ask: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text(model.message)
                .font(theme.type.body.font)
                .foregroundStyle(theme.text.primary.color)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel(model.message)
            ScrollView {
                VStack(alignment: .leading, spacing: theme.spacing.xs) {
                    ForEach(Array(model.lines.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(theme.type.body.font)
                            .foregroundStyle(theme.text.primary.color)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 60, maxHeight: 260)
            .accessibilityLabel("The changes: \(model.lines.joined(separator: "; "))")
            if model.offersAlways {
                Toggle(model.alwaysLabel, isOn: $model.always)
                    .toggleStyle(.checkbox)
                    .font(theme.type.body.font)
                    .foregroundStyle(theme.text.primary.color)
                    .accessibilityLabel(model.alwaysLabel)
                Text(model.alwaysLine)
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel(model.alwaysLine)
            } else {
                Text(model.unknownLine)
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
                    .accessibilityLabel(model.unknownLine)
            }
            HStack(spacing: theme.spacing.s) {
                Spacer()
                // Don't Allow is both Return and Esc: one shortcut per
                // button, so Esc's is an unseen twin.
                Button("Don't Allow") { model.deny() }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Don't Allow")
                    .background {
                        Button("Don't Allow") { model.deny() }
                            .keyboardShortcut(.cancelAction)
                            .opacity(0)
                            .accessibilityHidden(true)
                    }
                Button("Allow") { model.allow(realPress: Self.realPress(model, button: "Allow")) }
                    .accessibilityLabel("Allow")
            }
        }
    }

    private var warning: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text(model.warning)
                .font(theme.type.body.font)
                .foregroundStyle(theme.state.warning.color)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel(model.warning)
            HStack(spacing: theme.spacing.s) {
                Spacer()
                Button("Cancel") { model.cancelWarning() }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Cancel")
                    .background {
                        Button("Cancel") { model.cancelWarning() }
                            .keyboardShortcut(.cancelAction)
                            .opacity(0)
                            .accessibilityHidden(true)
                    }
                Button("Always Allow Anyway") {
                    model.allowAnyway(realPress: Self.realPress(model, button: "Always Allow Anyway"))
                }
                .accessibilityLabel("Always Allow Anyway")
            }
        }
    }
}

/// Tells the sheet's model which window it is in, for the press check.
private struct SheetWindowReader: NSViewRepresentable {
    let model: CoProducerApprovalSheetModel

    final class Probe: NSView {
        override init(frame: NSRect) {
            super.init(frame: frame)
            clipsToBounds = true
        }

        required init?(coder: NSCoder) { nil }

        var model: CoProducerApprovalSheetModel?
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            model?.window = window
        }
    }

    func makeNSView(context: Context) -> Probe {
        let p = Probe()
        p.model = model
        return p
    }

    func updateNSView(_ p: Probe, context: Context) {
        p.model = model
        model.window = p.window
    }
}

/// The app's approver (plan 8b Ruling 11): the sheet shows itself while
/// `DocumentModel.coProducerApproval` is set and answers through its model,
/// so asking only requests the composer's attention, once, with an
/// informational Dock bounce. It never brings VizDAW to the front.
@MainActor
final class SheetApprover: CoProducerApprover {
    static let shared = SheetApprover()

    func ask(_ pending: PendingApproval, done: @escaping @MainActor (ApprovalAnswer) -> Void) {
        // Under the developer channel (the e2e) nothing shows or bounces.
        guard !ControlChannel.active else { return }
        NSApp.requestUserAttention(.informationalRequest)
    }

    func cancel(doc: UUID) {}
}
