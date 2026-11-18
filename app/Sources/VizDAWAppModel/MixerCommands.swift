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
///
/// Plan 8d Ruling 1's thirteen Co-Producer commands are each one
/// `runMixer` call with the arguments as given (the core checks them), its
/// snapshot applied; each answers `status` with its title, and
/// `track.new_plugin_instrument` with the new track's name. On the
/// Co-Producer socket the twelve edits run only through
/// `co_producer.apply`, after its take (`CoProducerCommands`). Nothing
/// opens a sheet or a plug-in window.
@MainActor
public enum MixerCommands {
    public static let newBus = "mixer.new_bus"
    public static let newPluginInstrument = "track.new_plugin_instrument"
    /// Plan 8d Ruling 1's thirteen commands, in registry order.
    public static let coProducerEdits: [String] = [
        "mixer.set_strip", "mixer.set_output", "mixer.add_send", "mixer.set_send", "mixer.remove_send",
        "mixer.add_insert", "mixer.remove_insert", "mixer.move_insert", "mixer.set_bypass", "mixer.set_eq",
        "mixer.set_param", "mixer.remove_bus", "track.new_plugin_instrument",
    ]
    public static let ids: Set<String> = Set([newBus] + coProducerEdits)
    /// The registry's titles of `coProducerEdits`: each edit's status.
    public static let titles: [String: String] = [
        "mixer.set_strip": "Set Strip", "mixer.set_output": "Set Strip Output", "mixer.add_send": "Add Send",
        "mixer.set_send": "Set Send", "mixer.remove_send": "Remove Send", "mixer.add_insert": "Add Insert",
        "mixer.remove_insert": "Remove Insert", "mixer.move_insert": "Move Insert",
        "mixer.set_bypass": "Bypass Insert", "mixer.set_eq": "Set EQ", "mixer.set_param": "Set Plug-in Parameter",
        "mixer.remove_bus": "Remove Bus", newPluginInstrument: "New Plug-in Instrument Track",
    ]
    /// Why master gets no new bus.
    public static let masterHasNoSends = "Master has no sends."

    /// Runs `id` (one of `ids`) on document `docID`.
    public static func run(_ id: String, args: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        if coProducerEdits.contains(id) { return edit(id, args: args, docID: docID, m) }
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

    /// One of `coProducerEdits`: the gesture first, then the one session
    /// call. A refusal is `failed` in the session's words, its Busy `busy`.
    static func edit(_ id: String, args given: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        var args = given
        var seq: UInt64?
        var before: Set<UInt64> = []
        if id == newPluginInstrument {
            // The selected sequence and track, as New Track passes them.
            if args.contains(where: { $0.name == "seq" }) {
                seq = args.int("seq").flatMap { UInt64(exactly: $0) }
            } else if let s = m.selectedSeq, let value = Int64(exactly: s) {
                seq = s
                args.append(ControlArg(name: "seq", value: .int(value: value)))
            }
            if !args.contains(where: { $0.name == "after" }), let s = seq, s == m.selectedSeq,
                let lane = m.selectedLane?.id, let after = Int64(exactly: lane)
            {
                args.append(ControlArg(name: "after", value: .int(value: after)))
            }
            if let s = seq { before = Set(((try? m.session.lanes(seq: s)) ?? []).map(\.id)) }
        }
        do {
            m.apply(try m.session.runMixer(id: id, args: args))
        } catch {
            return .failed(CoProducerCommands.failure(error))
        }
        m.editor.status = titles[id]
        if id == newPluginInstrument, let s = seq,
            let made = ((try? m.session.lanes(seq: s)) ?? []).first(where: { !before.contains($0.id) })
        {
            m.editor.status = made.name
            // The new track is selected, as New Track selects it.
            if s == m.selectedSeq {
                m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(s, made.id).ref], events: []))
            }
        }
        return .done(ControlCommands.ran(id, docID, m))
    }
}
