import Foundation
import VizDAWCore

/// Document alerts while the control channel is on (control spec §7): no
/// sheet opens; each alert is reported once (stderr and `state.get
/// windows`) and stays on its document for `state.get document`.
public struct ControlAlertLog {
    private var reported: Set<UUID> = []

    public init() {}

    /// Whether a document's alert shows as a sheet.
    public static func showsSheet(_ alert: AlertItem?, channelOn: Bool) -> Bool {
        alert != nil && !channelOn
    }

    /// Of the documents' alerts now (`current`), those not reported yet.
    /// Alerts no longer present are forgotten, so the log stays small;
    /// a new alert is a new item (its own id) and is reported again.
    public mutating func unreported(_ current: [AlertItem]) -> [AlertItem] {
        var fresh: [AlertItem] = []
        var seen: Set<UUID> = []
        for a in current where seen.insert(a.id).inserted && !reported.contains(a.id) {
            fresh.append(a)
        }
        reported = seen
        return fresh
    }
}

/// Which window a control call names (control spec §5.3).
public enum ControlWindowCheck {
    /// Whether the call must answer `no_window`: `ref` is a window number
    /// that names no VizDAW window (`exists` says which numbers do).
    public static func namesNoWindow(_ ref: WindowRefRow?, exists: (Int64) -> Bool) -> Bool {
        if case .number(let n) = ref { return !exists(n) }
        return false
    }

    /// Whether a wait reads its window's document; the other conditions
    /// ignore `window`.
    public static func waitReadsDocument(_ c: WaitConditionRow) -> Bool {
        switch c {
        case .playing, .stopped, .tickAtLeast, .detailKind: true
        case .documentsAtLeast, .windowTitled, .windowGone: false
        }
    }
}
