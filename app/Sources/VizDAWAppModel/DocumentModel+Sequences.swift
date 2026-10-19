import Foundation
import VizDAWCore

/// A keyboard slot as menus show it ("⌃⌥⇧⌘A").
public enum KeySlotFormat {
    public static func text(_ slot: KeySlotRow?) -> String {
        guard let slot else { return "" }
        var s = ""
        if slot.modifiers & 2 != 0 { s += "⌃" }
        if slot.modifiers & 4 != 0 { s += "⌥" }
        if slot.modifiers & 1 != 0 { s += "⇧" }
        if slot.modifiers & 8 != 0 { s += "⌘" }
        return s + (slot.key.count == 1 ? slot.key.uppercased() : slot.key)
    }
}

/// The comment being edited.
public struct CommentDraft: Identifiable, Equatable, Sendable {
    public let seq: UInt64
    public var text: String
    public var id: UInt64 { seq }
}

/// The keyboard slot being edited.
public struct KeySlotDraft: Identifiable, Equatable, Sendable {
    public let seq: UInt64
    public var key: String
    public var shift = false
    public var control = false
    public var option = false
    public var command = false
    /// Why the last Set was refused (the key is a command's, plan 4f
    /// Ruling 15).
    public var problem: String?
    public var id: UInt64 { seq }

    var slot: KeySlotRow? {
        let k = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !k.isEmpty else { return nil }
        let m = (shift ? 1 : 0) | (control ? 2 : 0) | (option ? 4 : 0) | (command ? 8 : 0)
        return KeySlotRow(key: k, modifiers: UInt8(m))
    }
}

/// The sequence sidebar's intents (parent spec §9).
extension DocumentModel {
    /// Rows `source` of a sidebar section moved to `destination`.
    public func moveSequences(section: [SeqRow], from source: IndexSet, to destination: Int) {
        let order = SequenceOrder.moved(
            all: snapshot.sequences, order: editor.layout.sequenceOrder,
            visible: section.map(\.id), from: source, to: destination)
        editor.updateLayout { $0.sequenceOrder = order }
    }

    public func duplicate(_ seq: UInt64) {
        editor.edit(.sequence(edit: .duplicate(seq: seq)))
    }

    /// `base` or "`base` n" with the smallest n not taken.
    public func uniqueName(_ base: String) -> String {
        let taken = Set(snapshot.sequences.map(\.name))
        var n = 1
        while taken.contains("\(base) \(n)") { n += 1 }
        return "\(base) \(n)"
    }

    /// A new sequence from `template` (one undo step), then selected.
    public func newSequence(_ template: SequenceTemplate) {
        let before = Set(snapshot.sequences.map(\.id))
        let name = uniqueName("Sequence")
        do {
            apply(try session.newSequenceFromTemplate(name: name, template: template))
        } catch {
            editor.report(error)
            return
        }
        if let made = snapshot.sequences.first(where: { !before.contains($0.id) }) {
            choose(made.id)
        }
    }

    public func beginComment(_ seq: UInt64) {
        commentDraft = CommentDraft(
            seq: seq, text: snapshot.sequences.first { $0.id == seq }?.comment ?? "")
    }

    public func commitComment() {
        guard let d = commentDraft else { return }
        commentDraft = nil
        editor.edit(.sequence(edit: .setComment(seq: d.seq, comment: d.text)))
    }

    public func beginSlot(_ seq: UInt64) {
        let slot = snapshot.sequences.first { $0.id == seq }?.keyboardSlot
        var d = KeySlotDraft(seq: seq, key: slot?.key ?? "")
        let m = slot?.modifiers ?? 0
        d.shift = m & 1 != 0
        d.control = m & 2 != 0
        d.option = m & 4 != 0
        d.command = m & 8 != 0
        slotDraft = d
    }

    /// Sets the edited slot; a key that is a command's is refused and the
    /// draft stays open saying why (parent spec §5).
    public func commitSlot() {
        guard var d = slotDraft else { return }
        if let slot = d.slot, let clash = keys?.slotClash(slot) {
            d.problem = "\(KeySlotFormat.text(slot)) is the key of “\(clash)”."
            slotDraft = d
            return
        }
        slotDraft = nil
        editor.edit(.sequence(edit: .setKeyboardSlot(seq: d.seq, slot: d.slot)))
    }

    /// A sequence's keyboard slot was pressed (parent spec §5): during
    /// step entry it enters a reference to the sequence and moves on by
    /// its length; otherwise the sequence becomes the shown one and plays
    /// from its start.
    @discardableResult
    public func pressSlot(_ seq: UInt64) -> KeyOutcome {
        if stepEntry.isOn { return stepEntry.input(.reference(seq: seq)) }
        choose(seq)
        do {
            try startPlaying(from: 0)
            return .done
        } catch {
            return .refused(AlertItem(error, title: "").message)
        }
    }

    /// Deletes an unused sequence at once; a used one waits for
    /// `confirmDelete` (its references go too).
    public func requestDelete(_ seq: UInt64) {
        let uses = snapshot.sequences.first { $0.id == seq }?.useCount ?? 0
        if uses > 0 {
            pendingDelete = seq
        } else {
            editor.edit(.sequence(edit: .delete(seq: seq, deleteReferences: false)))
        }
    }

    public func confirmDelete() {
        guard let seq = pendingDelete else { return }
        pendingDelete = nil
        editor.edit(.sequence(edit: .delete(seq: seq, deleteReferences: true)))
    }
}
