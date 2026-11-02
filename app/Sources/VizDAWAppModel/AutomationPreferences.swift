import Foundation
import Observation
import VizDAWCore

/// The app's automation settings (sub-project 5 spec §7.5; plan 5e-2
/// Task 1): they belong to the machine, so they live in the preferences,
/// never in a project. Decoding is tolerant: a missing or damaged field
/// keeps its default.
public struct AutomationSettings: Codable, Equatable, Sendable {
    /// "off", "read", "touch", "latch" or "trim" (not "write").
    public var afterWrite = "latch"
    public var returnMs: UInt32 = 250
    /// "low", "medium" or "high".
    public var thinning = "medium"

    public init() {}

    enum CodingKeys: String, CodingKey { case afterWrite, returnMs, thinning }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = AutomationSettings()
        let a: String = c.field(.afterWrite, d.afterWrite)
        afterWrite = Self.afterWriteModes.contains(a) ? a : d.afterWrite
        let ms: UInt32 = c.field(.returnMs, d.returnMs)
        returnMs = ms <= Self.maxReturnMs ? ms : d.returnMs
        let t: String = c.field(.thinning, d.thinning)
        thinning = Self.thinnings.contains(t) ? t : d.thinning
    }

    public static let afterWriteModes = ["off", "read", "touch", "latch", "trim"]
    public static let maxReturnMs: UInt32 = 2000
    static let thinnings = ["low", "medium", "high"]

    /// The settings as the core spells them.
    public var row: AutomationSettingsRow {
        let mode: AutomationModeRow =
            switch afterWrite {
            case "off": .off
            case "read": .read
            case "touch": .touch
            case "trim": .trim
            default: .latch
            }
        let thin: ThinningRow =
            switch thinning {
            case "low": .low
            case "high": .high
            default: .medium
            }
        return AutomationSettingsRow(afterWrite: mode, returnMs: returnMs, thinning: thin)
    }
}

/// Where the settings go: the core's process-wide setter (the app's
/// `LiveAutomationSink`) or a test's fake.
@MainActor
public protocol AutomationSink: AnyObject {
    func apply(_ row: AutomationSettingsRow) throws
}

/// The automation settings the app keeps (plan 5e-2 Ruling 7): every
/// change goes to the core first and is kept only once it took; a refusal
/// leaves the setting as it was and says why in `problem`.
@Observable
@MainActor
public final class AutomationPreferences {
    public static let key = "VizDAW.automation"
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let sink: any AutomationSink
    public private(set) var settings: AutomationSettings
    /// Why the last change was refused (nil after one that took).
    public var problem: String?

    public init(defaults: UserDefaults, sink: any AutomationSink) {
        self.defaults = defaults
        self.sink = sink
        settings =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(AutomationSettings.self, from: $0) }
            ?? AutomationSettings()
    }

    /// Hands the settings to the core (at launch, before a document opens).
    public func applyAll() {
        do {
            try sink.apply(settings.row)
            problem = nil
        } catch {
            problem = AlertItem(error, title: "").message
        }
    }

    private func change(_ s: AutomationSettings) {
        do {
            try sink.apply(s.row)
            problem = nil
            settings = s
            if let data = try? JSONEncoder().encode(s) { defaults.set(data, forKey: Self.key) }
        } catch {
            problem = AlertItem(error, title: "").message
        }
    }

    public func setAfterWrite(_ mode: String) {
        guard AutomationSettings.afterWriteModes.contains(mode) else { return }
        var s = settings
        s.afterWrite = mode
        change(s)
    }

    /// The return time (ms), clamped to 0…2000.
    public func setReturnMs(_ ms: UInt32) {
        var s = settings
        s.returnMs = min(ms, AutomationSettings.maxReturnMs)
        change(s)
    }

    public func setThinning(_ t: String) {
        guard AutomationSettings.thinnings.contains(t) else { return }
        var s = settings
        s.thinning = t
        change(s)
    }
}
