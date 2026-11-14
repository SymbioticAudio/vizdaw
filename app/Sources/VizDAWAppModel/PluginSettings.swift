import Foundation
import Observation
import VizDAWCore

/// Where Settings › Plug-ins goes: the core's switch, its install and its
/// bundled-EQ facts (the app's live sink) or a test's fake.
@MainActor
public protocol PluginSettingsSink: AnyObject {
    func setNewChannelEq(_ on: Bool)
    func installMultiq(dest: String?, replace: Bool) throws -> String
    var bundledEqName: String? { get }
    var bundledAuPresent: Bool { get }
}

/// Settings › Plug-ins (plan 6g Task 4, Rulings 8–10): "Put an EQ on new
/// channels", a preference of this Mac pushed to the core at launch and on
/// every change, and "Install Multi-Q for other apps", which asks before
/// replacing (`confirmReplace`). The control command never asks: it goes
/// through `installQuietly`.
@Observable
@MainActor
public final class PluginSettings {
    public static let key = "VizDAW.newChannelEq"
    /// The line shown, and the install's answer, when the build has no EQ.
    public static let noBundledEq = "This build of VizDAW has no bundled EQ."
    /// The core's sentence for a build without the AU.
    public static let notInThisBuild = "Multi-Q is not in this build of VizDAW."

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let sink: any PluginSettingsSink
    /// The Components folder installs go to; nil (the app) is the core's
    /// default, `$HOME/Library/Audio/Plug-Ins/Components`.
    public let installDestination: String?
    /// The bundled EQ's name; nil when the build has none (both controls
    /// are then off).
    public let bundledEqName: String?
    /// The build has the AU the Install button copies.
    public let bundledAuPresent: Bool
    /// "Put an EQ on new channels" (default on).
    public private(set) var newChannelEq: Bool
    /// Set when an install found something in the way: the view asks
    /// whether to replace it.
    public var confirmReplace = false
    /// "Installed in <folder>." or why the install failed.
    public private(set) var status: String?

    public init(defaults: UserDefaults, sink: any PluginSettingsSink, installDestination: String? = nil) {
        self.defaults = defaults
        self.sink = sink
        self.installDestination = installDestination
        bundledEqName = sink.bundledEqName
        bundledAuPresent = sink.bundledAuPresent
        newChannelEq = defaults.object(forKey: Self.key) as? Bool ?? true
        sink.setNewChannelEq(newChannelEq)
    }

    /// Turns the switch on or off, keeps it, and tells the core.
    public func setNewChannelEq(_ on: Bool) {
        newChannelEq = on
        defaults.set(on, forKey: Self.key)
        sink.setNewChannelEq(on)
    }

    /// The Install button (and the Replace answer): something in the way
    /// sets `confirmReplace`; anything else lands in `status`.
    public func install(replace: Bool) {
        confirmReplace = false
        guard bundledEqName != nil else {
            status = Self.noBundledEq
            return
        }
        do {
            _ = try installQuietly(replace: replace)
        } catch VizError.Exists {
            status = nil
            confirmReplace = true
        } catch {
            status = (error as? VizError)?.message ?? error.localizedDescription
        }
    }

    /// Installs into `installDestination` and sets `status`; a failure is
    /// thrown and asks nothing. Returns the installed bundle's path.
    public func installQuietly(replace: Bool) throws -> String {
        let path = try sink.installMultiq(dest: installDestination, replace: replace)
        status = "Installed in \((path as NSString).deletingLastPathComponent)."
        return path
    }
}
