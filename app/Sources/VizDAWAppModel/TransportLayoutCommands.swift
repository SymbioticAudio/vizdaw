import Foundation
import VizDAWCore

/// `ui.set_transport_bar {items}` and `ui.reset_transport_bar`
/// (groups-vcas-transport Ruling 18): the channel's way to change the bar.
/// App-wide, so they need no document, and they open nothing.
@MainActor
public enum TransportLayoutCommands {
    public static let set = "ui.set_transport_bar"
    public static let reset = "ui.reset_transport_bar"
    public static let ids: Set<String> = [set, reset]

    /// From the channel: `items` is the `items` argument.
    public static func run(_ id: String, args: [ControlArg], store: TransportLayoutStore) -> ControlOutcome {
        run(id, items: args.text("items"), store: store)
    }

    public static func run(_ id: String, items: String?, store: TransportLayoutStore) -> ControlOutcome {
        switch id {
        case set:
            guard let items else {
                return .failed(.of(.invalidParams, "\(set): `items` lists the bar's items, such as \"cluster,loop\""))
            }
            switch TransportLayout.parse(items) {
            case .success(let l):
                store.set(l)
                return .done(ran(id, status: "transport bar: \(l.items.count) items"))
            case .failure(let e):
                return .failed(.of(.invalidParams, "\(set): \(e.message)"))
            }
        case reset:
            store.reset()
            return .done(ran(id, status: "transport bar: the default"))
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id.prefix(80))."))
        }
    }

    private static func ran(_ id: String, status: String) -> ControlResult {
        .ran(command: id, doc: nil, window: nil, undoLabel: nil, status: status)
    }
}
