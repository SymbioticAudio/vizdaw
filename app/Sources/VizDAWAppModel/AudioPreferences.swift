import Foundation
import Observation
import VizDAWCore

/// The app's audio and recording settings (sub-project 5 spec §6.9
/// Settings › Audio and › Recording; plan 5d-2 Ruling 2): they belong to
/// the machine, so they live in the preferences, never in a project.
/// Decoding is tolerant: a missing or damaged field keeps its default.
public struct AudioSettings: Codable, Equatable, Sendable {
    public var softwareMonitoring = true
    public var bufferFrames: UInt32 = 128
    /// The input device by name; nil: "Same as output".
    public var inputDevice: String?
    public var latencyTrim: Int32 = 0
    public var cacheMb: UInt32 = 512
    public var readAheadSeconds: UInt32 = 3
    /// "auto", "input" or "off".
    public var defaultMonitor = "auto"
    public var countInBars: UInt8 = 0
    public var preRollBars: UInt8 = 0
    public var passesAsTakes = true

    public init() {}

    enum CodingKeys: String, CodingKey {
        case softwareMonitoring, bufferFrames, inputDevice, latencyTrim, cacheMb, readAheadSeconds
        case defaultMonitor, countInBars, preRollBars, passesAsTakes
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = AudioSettings()
        softwareMonitoring = c.field(.softwareMonitoring, d.softwareMonitoring)
        let b: UInt32 = c.field(.bufferFrames, d.bufferFrames)
        bufferFrames = Self.bufferSizes.contains(b) ? b : d.bufferFrames
        inputDevice = c.field(.inputDevice, d.inputDevice)
        let t: Int32 = c.field(.latencyTrim, d.latencyTrim)
        latencyTrim = Int64(t).magnitude <= UInt64(Self.maxTrim) ? t : 0
        let mb: UInt32 = c.field(.cacheMb, d.cacheMb)
        cacheMb = (64...8192).contains(mb) ? mb : d.cacheMb
        let s: UInt32 = c.field(.readAheadSeconds, d.readAheadSeconds)
        readAheadSeconds = (2...8).contains(s) ? s : d.readAheadSeconds
        let m: String = c.field(.defaultMonitor, d.defaultMonitor)
        defaultMonitor = AudioSettings.monitors.contains(m) ? m : d.defaultMonitor
        countInBars = min(c.field(.countInBars, d.countInBars), 4)
        preRollBars = min(c.field(.preRollBars, d.preRollBars), 8)
        passesAsTakes = c.field(.passesAsTakes, d.passesAsTakes)
    }

    static let monitors = ["auto", "input", "off"]
    /// `AudioCommands.bufferSizes` and `maxTrim`, readable off the main
    /// actor (the decoder).
    static let bufferSizes: [UInt32] = [32, 64, 128, 256, 512, 1024]
    static let maxTrim: Int64 = 192_000

    /// The default monitor mode as the core spells it.
    public var monitorRow: MonitorModeRow {
        switch defaultMonitor {
        case "input": .input
        case "off": .off
        default: .auto
        }
    }
}

/// Where the settings go: the core's process-wide setters (the app's
/// `LiveAudioSink`) or a test's fake.
@MainActor
public protocol AudioSink: AnyObject {
    func softwareMonitoring(_ on: Bool)
    /// `Busy` while recording.
    func bufferFrames(_ frames: UInt32) throws
    func inputDevice(_ name: String?)
    func latencyTrim(_ frames: Int32)
    func stream(cacheMb: UInt32, readAheadSeconds: UInt32) throws
    func defaultMonitor(_ mode: MonitorModeRow)
}

/// The audio settings the app keeps (plan 5d-2 Ruling 2): every change
/// goes to the core first and is kept only once it took; a refusal (the
/// buffer size while recording) leaves the setting as it was and says why
/// in `problem`.
@Observable
@MainActor
public final class AudioPreferences {
    public static let key = "VizDAW.audio"
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let sink: any AudioSink
    public private(set) var settings: AudioSettings
    /// Why the last change was refused (nil after one that took).
    public var problem: String?
    /// The device's input as Settings › Audio last read it (Task 7).
    public internal(set) var device: InputInfoRow?

    public init(defaults: UserDefaults, sink: any AudioSink) {
        self.defaults = defaults
        self.sink = sink
        settings =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(AudioSettings.self, from: $0) }
            ?? AudioSettings()
    }

    /// Hands every setting to the core (at launch, before a document
    /// opens).
    public func applyAll() {
        let s = settings
        sink.softwareMonitoring(s.softwareMonitoring)
        sink.inputDevice(s.inputDevice)
        sink.latencyTrim(s.latencyTrim)
        sink.defaultMonitor(s.monitorRow)
        RecordModel.defaults = RecordDefaults(s)
        do {
            try sink.bufferFrames(s.bufferFrames)
            try sink.stream(cacheMb: s.cacheMb, readAheadSeconds: s.readAheadSeconds)
            problem = nil
        } catch {
            problem = AlertItem(error, title: "").message
        }
    }

    private func save(_ s: AudioSettings) {
        settings = s
        if let data = try? JSONEncoder().encode(s) { defaults.set(data, forKey: Self.key) }
    }

    /// `audio.set` (plan 5d-1 Ruling 26): the buffer size first (it is the
    /// one that can be refused), then the rest; nothing changes when it is.
    public func set(_ c: AudioChange) throws {
        var s = settings
        if let b = c.bufferFrames {
            try sink.bufferFrames(b)
            s.bufferFrames = b
        }
        if let on = c.softwareMonitoring {
            sink.softwareMonitoring(on)
            s.softwareMonitoring = on
        }
        if let d = c.inputDevice {
            sink.inputDevice(d)
            s.inputDevice = d
        }
        if let t = c.latencyTrim {
            sink.latencyTrim(t)
            s.latencyTrim = t
        }
        problem = nil
        save(s)
    }

    /// A Settings control's change; a refusal goes to `problem`.
    private func change(_ c: AudioChange) {
        do {
            try set(c)
        } catch {
            problem = AlertItem(error, title: "").message
        }
    }

    public func setSoftwareMonitoring(_ on: Bool) { change(AudioChange(softwareMonitoring: on)) }
    public func setBufferFrames(_ frames: UInt32) { change(AudioChange(bufferFrames: frames)) }
    public func setInputDevice(_ name: String?) { change(AudioChange(inputDevice: .some(name))) }
    public func setLatencyTrim(_ frames: Int32) {
        let t = max(-AudioSettings.maxTrim, min(AudioSettings.maxTrim, Int64(frames)))
        change(AudioChange(latencyTrim: Int32(clamping: t)))
    }

    /// The RAM cache (MiB) and the disk read-ahead (s), clamped to their
    /// ranges.
    public func setStream(cacheMb: UInt32, readAheadSeconds: UInt32) {
        var s = settings
        s.cacheMb = min(max(cacheMb, 64), 8192)
        s.readAheadSeconds = min(max(readAheadSeconds, 2), 8)
        do {
            try sink.stream(cacheMb: s.cacheMb, readAheadSeconds: s.readAheadSeconds)
            problem = nil
            save(s)
        } catch {
            problem = AlertItem(error, title: "").message
        }
    }

    public func setDefaultMonitor(_ mode: String) {
        guard AudioSettings.monitors.contains(mode) else { return }
        var s = settings
        s.defaultMonitor = mode
        sink.defaultMonitor(s.monitorRow)
        save(s)
    }

    /// Settings › Recording's defaults for new documents' record options.
    public func setRecordDefaults(countIn: UInt8, preRoll: UInt8, passesAsTakes: Bool) {
        var s = settings
        s.countInBars = min(countIn, 4)
        s.preRollBars = min(preRoll, 8)
        s.passesAsTakes = passesAsTakes
        save(s)
        RecordModel.defaults = RecordDefaults(s)
    }
}

/// What a new document's record options start as (Settings › Recording;
/// plan 5d-2 Ruling 3).
public struct RecordDefaults: Equatable, Sendable {
    public var countInBars: UInt8 = 0
    public var preRollBars: UInt8 = 0
    public var passesAsTakes = true

    public init() {}

    public init(_ s: AudioSettings) {
        countInBars = s.countInBars
        preRollBars = s.preRollBars
        passesAsTakes = s.passesAsTakes
    }
}
