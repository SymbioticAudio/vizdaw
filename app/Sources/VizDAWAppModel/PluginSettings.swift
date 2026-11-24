import Foundation
import Observation
import VizDAWCore

/// Where Settings › Plug-ins goes: the core's switch, its install, its
/// bundled-EQ facts and the chosen EQ (the app's live sink) or a test's fake.
@MainActor
public protocol PluginSettingsSink: AnyObject {
    func setNewChannelEq(_ on: Bool)
    /// The EQ new channels get; nil is Multi-Q (plan 6g-2 Ruling 5).
    func setDefaultEq(_ choice: DefaultEqRow?)
    /// The installed effects the picker offers (Ruling 6), Multi-Q's CLAP
    /// excluded.
    func eqChoices() -> [DefaultEqRow]
    /// The core's line on the choice: nil when it is usable, unset, or the
    /// switch is off (Ruling 4).
    func defaultEqNote() -> String?
    func installMultiq(dest: String?, replace: Bool) throws -> String
    var bundledEqName: String? { get }
    var bundledAuPresent: Bool { get }
}

/// Settings › Plug-ins (plan 6g Task 4, Rulings 8–10): "Put an EQ on new
/// channels", a preference of this Mac pushed to the core at launch and on
/// every change, and "Install Multi-Q for other apps", which asks before
/// replacing (`confirmReplace`). The control command never asks: it goes
/// through `installQuietly`. Plan 6g-2 adds the picker of which EQ that is
/// (Rulings 5–7): Multi-Q, or an installed AU or CLAP effect.
@Observable
@MainActor
public final class PluginSettings {
    public static let key = "VizDAW.newChannelEq"
    /// The line shown, and the install's answer, when the build has no EQ.
    public static let noBundledEq = "This build of VizDAW has no bundled EQ."
    /// The core's sentence for a build without the AU.
    public static let notInThisBuild = "Multi-Q is not in this build of VizDAW."
    /// Where the chosen EQ is kept: `["format": "au"|"clap", "id", "name"]`;
    /// absent is Multi-Q (plan 6g-2 Ruling 5).
    public static let eqKey = "VizDAW.newChannelEqPlugin"
    /// Multi-Q's own CLAP ID, never offered as a second item (Ruling 5).
    public static let multiqClapId = "com.dusk-audio.multi-q"

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
    /// The EQ new channels get; nil is Multi-Q.
    public private(set) var defaultEq: DefaultEqRow?
    /// The installed effects the picker offers, read by `refreshEqChoices`.
    public private(set) var eqChoices: [DefaultEqRow] = []
    /// The core's line under the picker (the fallback), or nil.
    public private(set) var eqNote: String?

    /// The name the caption gives the EQ new channels get.
    public var defaultEqName: String { defaultEq?.name ?? "Multi-Q" }

    /// The choice is Multi-Q, or one of `eqChoices` (matched by format and ID).
    public var defaultEqAvailable: Bool {
        guard let c = defaultEq else { return true }
        return eqChoices.contains { $0.format == c.format && $0.id == c.id }
    }

    public init(defaults: UserDefaults, sink: any PluginSettingsSink, installDestination: String? = nil) {
        self.defaults = defaults
        self.sink = sink
        self.installDestination = installDestination
        bundledEqName = sink.bundledEqName
        bundledAuPresent = sink.bundledAuPresent
        newChannelEq = defaults.object(forKey: Self.key) as? Bool ?? true
        sink.setNewChannelEq(newChannelEq)
        defaultEq = Self.readEq(defaults)
        sink.setDefaultEq(defaultEq)
    }

    /// Turns the switch on or off, keeps it, and tells the core.
    public func setNewChannelEq(_ on: Bool) {
        newChannelEq = on
        defaults.set(on, forKey: Self.key)
        sink.setNewChannelEq(on)
        eqNote = sink.defaultEqNote()
    }

    /// Chooses the EQ new channels get (nil: Multi-Q, which stores nothing),
    /// keeps it, tells the core and reads its note again.
    public func setDefaultEq(_ choice: DefaultEqRow?) {
        defaultEq = choice
        if let c = choice {
            defaults.set(["format": Self.formatName(c.format), "id": c.id, "name": c.name], forKey: Self.eqKey)
        } else {
            defaults.removeObject(forKey: Self.eqKey)
        }
        sink.setDefaultEq(choice)
        eqNote = sink.defaultEqNote()
    }

    /// Reads the installed effects and the core's note again (the tab
    /// appearing, a command about to match against them).
    public func refreshEqChoices() {
        eqChoices = sink.eqChoices()
        eqNote = sink.defaultEqNote()
    }

    /// Ruling 6's installed items from the catalog's effect browser: usable
    /// and enabled ones, Multi-Q's CLAP left out.
    public static func eqChoices(from rows: [CatalogRowFfi]) -> [DefaultEqRow] {
        rows.compactMap { r in
            guard r.kind == .effect, r.enabled, !r.plugin.id.isEmpty else { return nil }
            switch r.status {
            case .ok, .notValidated: break
            default: return nil
            }
            if r.plugin.format == .clap && r.plugin.id == multiqClapId { return nil }
            return DefaultEqRow(format: r.plugin.format, id: r.plugin.id, name: r.plugin.name)
        }
    }

    /// The format's name in the stored choice and the command's prefix.
    public static func formatName(_ f: PluginFormatTag) -> String {
        switch f {
        case .au: "au"
        case .clap: "clap"
        case .vst3: "vst3"
        }
    }

    /// The stored choice; any other shape is Multi-Q, and the key is removed.
    private static func readEq(_ defaults: UserDefaults) -> DefaultEqRow? {
        guard let value = defaults.object(forKey: eqKey) else { return nil }
        let d = value as? [String: Any]
        let format: PluginFormatTag? =
            switch d?["format"] as? String ?? "" {
            case "au": .au
            case "clap": .clap
            default: nil
            }
        guard let format, let id = d?["id"] as? String, let name = d?["name"] as? String,
            !id.isEmpty, !id.contains("\u{0}"), !name.contains("\u{0}")
        else {
            defaults.removeObject(forKey: eqKey)
            return nil
        }
        return DefaultEqRow(format: format, id: id, name: name)
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
