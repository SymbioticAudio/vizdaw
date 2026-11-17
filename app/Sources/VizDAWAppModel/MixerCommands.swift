import Foundation
import VizDAWCore

/// `mixer.new_bus` (plan 6g-2 Task 3, Ruling 7): with `strip`, a new bus
/// and a post-fader 0 dB send to it from that strip, one undo step "New
/// Bus"; without, a bus alone ("Add Bus"). `name` defaults to the next
/// free "Bus N" (`MixerModel.nextBusName`). It answers `status` with the
/// bus's name.
///
/// Errors: a `strip` that is not an ID, a NUL in `name`, a blank `name`
/// and master as `strip` ("Master has no sends.") are `invalid_params`; a
/// strip the mixer does not have is `unknown_id`. None of them reaches the
/// session. The core's own refusals are `failed`, its `Busy` is `busy`.
/// Nothing here opens a dialog.
@MainActor
public enum MixerCommands {
    public static let newBus = "mixer.new_bus"
    /// Plan 8d Ruling 1's thirteen commands, in registry order. They answer
    /// "Not ready yet." until plan 8d Task 5.
    public static let coProducerEdits: [String] = [
        "mixer.set_strip", "mixer.set_output", "mixer.add_send", "mixer.set_send", "mixer.remove_send",
        "mixer.add_insert", "mixer.remove_insert", "mixer.move_insert", "mixer.set_bypass", "mixer.set_eq",
        "mixer.set_param", "mixer.remove_bus", "track.new_plugin_instrument",
    ]
    public static let ids: Set<String> = Set([newBus] + coProducerEdits)
    /// Why master gets no new bus.
    public static let masterHasNoSends = "Master has no sends."

    /// Runs `id` (one of `ids`) on document `docID`.
    public static func run(_ id: String, args: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        if coProducerEdits.contains(id) { return .failed(.of(.unavailable, "Not ready yet.")) }
        guard id == newBus else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        var strip: UInt64?
        if args.contains(where: { $0.name == "strip" }) {
            guard let s = args.int("strip").flatMap({ UInt64(exactly: $0) }) else {
                return .failed(.of(.invalidParams, "mixer.new_bus: `strip` is a strip ID (see state.get mixer)"))
            }
            strip = s
        }
        var given: String?
        if args.contains(where: { $0.name == "name" }) {
            guard let text = args.text("name"), !text.contains("\u{0}") else {
                return .failed(.of(.invalidParams, "mixer.new_bus: `name` is text without a NUL"))
            }
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else {
                return .failed(.of(.invalidParams, "mixer.new_bus: `name` is not blank"))
            }
            given = trimmed
        }
        do {
            let mixer = try m.session.mixer()
            if let strip {
                guard let row = mixer.strips.first(where: { $0.id == strip }) else {
                    return .failed(.of(.unknownId, "The mixer has no strip \(strip)."))
                }
                if row.kind == .master { return .failed(.of(.invalidParams, masterHasNoSends)) }
            }
            let name = given ?? MixerModel.nextBusName(mixer.buses)
            if let strip {
                m.apply(try m.session.newBusFrom(strip: strip, name: name))
            } else {
                m.apply(try m.session.addBus(name: name))
            }
            m.editor.status = name
        } catch {
            return .failed(.thrown(error, "New Bus Failed"))
        }
        return .done(ControlCommands.ran(id, docID, m))
    }
}
