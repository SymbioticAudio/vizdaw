import Foundation
import VizDAWCore

/// Automation targets and values as text where no mixer names are at hand
/// (the list editor, Plan 4d Ruling 26): the Strip Chart's naming without
/// names (`StripNames`, `AutomationLaw`), so the two never drift apart.
public enum AutomationText {
    public static func short(_ t: AutomationTargetRow) -> String { StripNames().title(AutoTarget(t)) }

    /// A normalised value as 0.0–100.0 %.
    public static func percent(_ v: Double) -> String { AutomationLaw.percent(v) }

    /// Typed "0"…"100" (an optional "%") as a normalised value.
    public static func parsePercent(_ s: String) -> Double? {
        let t = s.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
        guard let p = Double(t), p.isFinite, (0...100).contains(p) else { return nil }
        return p / 100
    }
}
