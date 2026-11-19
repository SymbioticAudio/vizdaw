import SwiftUI
import SymbioticTokens
import VizDAWAppModel
import VizDAWCore

/// View › Customize Transport Bar… (groups-vcas-transport Task 12, Ruling
/// 15): the bar's draft and a palette. Items drag onto the bar, along it
/// and back to the palette; every item has the same moves as accessibility
/// actions. The inspector under the bar sets a readout's size and a
/// command button's command and label. Done writes the bar; Cancel leaves
/// it. All state is `TransportCustomizeModel`'s (no `@State`).
struct TransportCustomizeSheet: View {
    @Environment(\.symbioticTheme) private var theme
    @Bindable var model: TransportCustomizeModel
    let keys: KeyBindings

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.m) {
            Text("Customize Transport Bar").font(theme.type.title.font)
                .accessibilityAddTraits(.isHeader)
            Text("Drag items onto the bar, along it, or back to the palette to remove them.")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
            section("Bar") { bar }
            inspector
            section("Palette") { palette }
            HStack {
                Button("Reset to Default") { model.resetToDefault() }
                    .accessibilityLabel("Reset to Default")
                Spacer()
                Button("Cancel") { model.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityLabel("Cancel")
                Button("Done") { model.apply() }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityLabel("Done")
            }
        }
        .padding(theme.spacing.l)
        .frame(width: 760)
        .frame(minHeight: 480)
        .background(theme.surface.window.color)
    }

    private func section<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            Text(title).font(theme.type.label.font).foregroundStyle(theme.text.secondary.color)
                .accessibilityAddTraits(.isHeader)
            content()
        }
    }

    // MARK: The bar

    private var bar: some View {
        let entries = TransportCustomizeEntries.bar(model.draft.items) { keys.row($0) }
        let count = entries.count
        return ScrollView(.horizontal) {
            HStack(spacing: theme.spacing.xs) {
                ForEach(Array(entries.enumerated()), id: \.offset) { n, e in
                    barChip(n, e, count: count)
                }
                // The end of the bar: a drop here appends.
                Color.clear.frame(width: 40, height: 28)
                    .contentShape(Rectangle())
                    .dropDestination(for: String.self) { tokens, _ in
                        drop(tokens, at: model.draft.items.count)
                    }
                    .accessibilityHidden(true)
            }
            .padding(theme.spacing.s)
        }
        .background(theme.surface.inset.color)
        .clipShape(RoundedRectangle(cornerRadius: theme.radius.s))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Transport Bar Items")
    }

    private func barChip(_ n: Int, _ e: TransportCustomizeEntries.Entry, count: Int) -> some View {
        let m = model
        return chip(e.title, selected: m.selected == n)
            .onTapGesture { m.select(m.selected == n ? nil : n) }
            .draggable(TransportCustomizeEntries.token(bar: n)) { chip(e.title, selected: true) }
            .dropDestination(for: String.self) { tokens, _ in drop(tokens, at: n) }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(e.accessibilityLabel)
            .accessibilityAddTraits(.isButton)
            .accessibilityAddTraits(m.selected == n ? .isSelected : [])
            .accessibilityAction { m.select(n) }
            .accessibilityAction(named: TransportCustomizeEntries.removeAction) {
                if e.actions.contains(TransportCustomizeEntries.removeAction) { m.remove(at: n) }
            }
            .accessibilityAction(named: TransportCustomizeEntries.leftAction) {
                if n > 0 { m.move(from: n, to: n - 1) }
            }
            .accessibilityAction(named: TransportCustomizeEntries.rightAction) {
                if n + 1 < count { m.move(from: n, to: n + 1) }
            }
            .contextMenu {
                Button(TransportCustomizeEntries.removeAction) { m.remove(at: n) }
                    .disabled(!e.actions.contains(TransportCustomizeEntries.removeAction))
                Button(TransportCustomizeEntries.leftAction) { m.move(from: n, to: n - 1) }
                    .disabled(n == 0)
                Button(TransportCustomizeEntries.rightAction) { m.move(from: n, to: n + 1) }
                    .disabled(n + 1 >= count)
            }
    }

    private func drop(_ tokens: [String], at index: Int) -> Bool {
        guard let t = tokens.first else { return false }
        return TransportCustomizeEntries.drop(t, onBarAt: index, model)
    }

    // MARK: The palette

    private var palette: some View {
        let items = model.palette
        let entries = TransportCustomizeEntries.palette(items)
        let m = model
        return LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), spacing: theme.spacing.xs)],
                         alignment: .leading, spacing: theme.spacing.xs) {
            ForEach(Array(entries.enumerated()), id: \.offset) { n, e in
                chip(e.title, selected: false)
                    .onTapGesture(count: 2) { m.add(items[n], at: m.draft.items.count) }
                    .draggable(TransportCustomizeEntries.token(palette: n)) { chip(e.title, selected: true) }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(e.accessibilityLabel)
                    .accessibilityAddTraits(.isButton)
                    .accessibilityAction { m.add(items[n], at: m.draft.items.count) }
                    .accessibilityAction(named: TransportCustomizeEntries.addAction) {
                        m.add(items[n], at: m.draft.items.count)
                    }
                    .contextMenu {
                        Button(TransportCustomizeEntries.addAction) { m.add(items[n], at: m.draft.items.count) }
                    }
            }
        }
        .padding(theme.spacing.s)
        .background(theme.surface.inset.color)
        .clipShape(RoundedRectangle(cornerRadius: theme.radius.s))
        .dropDestination(for: String.self) { tokens, _ in
            guard let t = tokens.first else { return false }
            return TransportCustomizeEntries.dropOnPalette(t, m)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Palette")
    }

    private func chip(_ title: String, selected: Bool) -> some View {
        Text(title)
            .font(theme.type.body.font)
            .foregroundStyle(theme.text.primary.color)
            .lineLimit(1)
            .padding(.horizontal, theme.spacing.s)
            .padding(.vertical, theme.spacing.xs)
            .background(theme.surface.raised.color, in: RoundedRectangle(cornerRadius: theme.radius.s))
            .overlay(
                RoundedRectangle(cornerRadius: theme.radius.s)
                    .stroke(selected ? theme.accent.accent.color : theme.surface.separator.color,
                            lineWidth: selected ? 2 : 1))
    }

    // MARK: The inspector

    @ViewBuilder private var inspector: some View {
        if let n = model.selected, model.draft.items.indices.contains(n) {
            let item = model.draft.items[n]
            switch item {
            case .builtIn(let id, _) where id.sizes:
                sizePicker(n, item)
            case .command(let cmd, _):
                commandEditor(n, cmd)
            default:
                Text("\(TransportCustomizeEntries.title(item)) has no settings.")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.text.secondary.color)
            }
        } else {
            Text("Select an item on the bar to set its size or its command.")
                .font(theme.type.caption.font)
                .foregroundStyle(theme.text.secondary.color)
        }
    }

    private func sizePicker(_ n: Int, _ item: TransportItem) -> some View {
        let m = model
        return Picker(
            "\(item.id.title) Size",
            selection: Binding(get: { m.draft.items.indices.contains(n) ? m.draft.items[n].size : .large },
                               set: { m.setSize(at: n, $0) })
        ) {
            Text("Small").tag(ReadoutSize.small)
            Text("Large").tag(ReadoutSize.large)
        }
        .pickerStyle(.segmented)
        .frame(width: 320)
        .accessibilityLabel("\(item.id.title) Size")
    }

    private func commandEditor(_ n: Int, _ cmd: String) -> some View {
        let m = model
        let rows = CommandPicker.rows(keys.rows, query: m.query)
        let tooLong = m.labelDraft.trimmingCharacters(in: .whitespacesAndNewlines).count > TransportItem.maxLabel
        return VStack(alignment: .leading, spacing: theme.spacing.xs) {
            HStack(spacing: theme.spacing.s) {
                TextField("Search Commands", text: $model.query)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 260)
                    .accessibilityLabel("Search Commands")
                TextField("Label (the command's title when empty)", text: $model.labelDraft)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { m.commitLabel() }
                    .accessibilityLabel("Button Label")
                Button("Set Label") { m.commitLabel() }
                    .disabled(tooLong)
                    .accessibilityLabel("Set Label")
            }
            if tooLong {
                Text("A label is at most \(TransportItem.maxLabel) characters.")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.state.error.color)
            }
            List(rows, id: \.id) { row in
                Button {
                    m.chooseCommand(row.id)
                } label: {
                    HStack {
                        Text(row.title).foregroundStyle(theme.text.primary.color)
                        Text(row.menu).foregroundStyle(theme.text.secondary.color)
                        Spacer()
                        if row.id == cmd {
                            AppIcon.confirm.image.foregroundStyle(theme.accent.accent.color)
                                .accessibilityHidden(true)
                        }
                    }
                    .font(theme.type.body.font)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(row.title), \(row.menu)")
                .accessibilityAddTraits(row.id == cmd ? .isSelected : [])
            }
            .frame(height: 160)
            .accessibilityLabel("Commands")
            if keys.row(cmd) == nil {
                Text("\(cmd): \(CommandButtonAction.staleTooltip).")
                    .font(theme.type.caption.font)
                    .foregroundStyle(theme.state.warning.color)
            }
        }
    }
}
