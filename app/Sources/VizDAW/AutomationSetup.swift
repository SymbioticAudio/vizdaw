import Foundation
import VizDAWAppModel
import VizDAWCore

/// The core's process-wide automation settings (plan 5e-2 Task 1).
@MainActor
final class LiveAutomationSink: AutomationSink {
    func apply(_ row: AutomationSettingsRow) throws { try setAutomationSettings(settings: row) }
}

/// The automation settings (Settings › Automation), in memory under
/// `VIZDAW_SUPPORT_DIR` (vizctl, tests).
@MainActor let automationPreferences = AutomationPreferences(
    defaults: MemoryDefaults.appStore(), sink: LiveAutomationSink())
