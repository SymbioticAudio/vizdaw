import Foundation
import VizDAWCore

/// Automation targets and values as text where no mixer names are at hand
/// (the list editor, Plan 4d Ruling 26).
public enum AutomationText {
    public static func short(_ t: AutomationTargetRow) -> String {
        switch t {
        case .stripFader(let s): "Fader · strip \(s)"
        case .stripPan(let s): "Pan · strip \(s)"
        case .stripMute(let s): "Mute · strip \(s)"
        case .stripSolo(let s): "Solo · strip \(s)"
        case .sendLevel(let s, let i): "Send \(Int(i) + 1) · strip \(s)"
        case .pluginParam(let slot, let p): "Parameter \(p) · plugin \(slot)"
        case .velocityScale(let d): "Velocity Scale · destination \(d)"
        }
    }

    /// A normalised value as 0.0–100.0 %.
    public static func percent(_ v: Double) -> String {
        String(format: "%.1f%%", min(max(v.isNaN ? 0 : v, 0), 1) * 100)
    }

    /// Typed "0"…"100" (an optional "%") as a normalised value.
    public static func parsePercent(_ s: String) -> Double? {
        let t = s.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
        guard let p = Double(t), p.isFinite, (0...100).contains(p) else { return nil }
        return p / 100
    }
}
