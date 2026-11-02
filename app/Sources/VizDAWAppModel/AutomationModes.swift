import VizDAWCore

/// The automation modes as the mixer shows them (plan 5e-2 Rulings 4–6):
/// their order, titles and colours.
public enum AutomationModes {
    /// Every mode in menu order: Off, Read, Touch, Latch, Write, Trim.
    public static let all: [AutomationModeRow] = [.off, .read, .touch, .latch, .write, .trim]

    /// "Off" … "Trim".
    nonisolated public static func title(_ m: AutomationModeRow) -> String {
        switch m {
        case .off: "Off"
        case .read: "Read"
        case .touch: "Touch"
        case .latch: "Latch"
        case .write: "Write"
        case .trim: "Trim"
        }
    }

    /// Ruling 4: "gray", "green", "yellow", "orange", "red", "purple" (the
    /// view maps them to system colours).
    nonisolated public static func colorName(_ m: AutomationModeRow) -> String {
        switch m {
        case .off: "gray"
        case .read: "green"
        case .touch: "yellow"
        case .latch: "orange"
        case .write: "red"
        case .trim: "purple"
        }
    }
}
