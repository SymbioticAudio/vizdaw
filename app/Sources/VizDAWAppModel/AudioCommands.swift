import Foundation
import VizDAWCore

/// What `audio.set` changes (plan 5d-1 Ruling 26): each field set is one
/// setting, nil leaves it. `inputDevice` set to `.some(nil)` is "Same as
/// output".
public struct AudioChange: Equatable, Sendable {
    public var softwareMonitoring: Bool?
    public var bufferFrames: UInt32?
    public var inputDevice: String??
    public var latencyTrim: Int32?

    public init(
        softwareMonitoring: Bool? = nil, bufferFrames: UInt32? = nil, inputDevice: String?? = nil,
        latencyTrim: Int32? = nil
    ) {
        self.softwareMonitoring = softwareMonitoring
        self.bufferFrames = bufferFrames
        self.inputDevice = inputDevice
        self.latencyTrim = latencyTrim
    }

    /// Nothing set.
    public var isEmpty: Bool { self == AudioChange() }
}

/// The audio settings and new MIDI destinations from the control channel
/// (plan 5d-1 Rulings 25–26): `audio.set` (app-wide, no document) and
/// `destination.add_midi`. Values of the wrong shape or range are
/// `invalid_params` and never reach the core; the core's refusals (a
/// buffer change while recording is `busy`) are the call's failure. No
/// dialog opens.
@MainActor
public enum AudioCommands {
    public static let set = "audio.set"
    public static let addMidi = "destination.add_midi"
    public static let ids: Set<String> = [set, addMidi]

    /// The I/O buffer sizes (spec §6.6).
    public static let bufferSizes: [UInt32] = [32, 64, 128, 256, 512, 1024]
    /// The latency trim's range in frames (5c `MAX_TRIM`).
    public static let maxTrim: Int64 = 192_000

    /// `audio.set`'s arguments as a change; nil for a value out of range
    /// or no value at all.
    public static func change(_ args: [ControlArg]) -> AudioChange? {
        var c = AudioChange()
        c.softwareMonitoring = args.flag("software_monitoring")
        if let n = args.int("buffer_frames") {
            guard let f = UInt32(exactly: n), bufferSizes.contains(f) else { return nil }
            c.bufferFrames = f
        }
        if let name = args.text("input_device") {
            let t = name.trimmingCharacters(in: .whitespacesAndNewlines)
            guard t.count <= 255, !t.contains(where: { $0.isNewline }) else { return nil }
            c.inputDevice = .some(t.isEmpty ? nil : t)
        }
        if let n = args.int("latency_trim") {
            guard (-maxTrim...maxTrim).contains(n) else { return nil }
            c.latencyTrim = Int32(n)
        }
        return c.isEmpty ? nil : c
    }

    /// `audio.set`: applies `args` through `host` (the app's preferences).
    public static func runSet(_ args: [ControlArg], host: any ControlHost) -> ControlOutcome {
        guard let c = change(args) else {
            return .failed(
                .of(
                    .invalidParams,
                    "audio.set: give software_monitoring, buffer_frames (32, 64, 128, 256, 512 or 1024), input_device or latency_trim (±192000)"
                ))
        }
        do {
            try host.applyAudio(c)
        } catch {
            return .failed(.thrown(error, "Audio Settings Failed"))
        }
        return .done(ControlCommands.ran(set, nil))
    }

    /// `destination.add_midi`: a destination for a MIDI port, one undo step.
    public static func runAddMidi(_ args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        let name = args.text("name")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let port = args.text("port")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let channel = args.int("channel") ?? 1
        guard !name.isEmpty, !port.isEmpty, name.count <= 255, port.count <= 255, (1...16).contains(channel) else {
            return .failed(
                .of(
                    .invalidParams,
                    "destination.add_midi: `name` and `port` are 1 to 255 characters, `channel` 1 to 16"))
        }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        do {
            m.apply(try m.session.addMidiDestination(name: name, port: port, channel: UInt8(channel - 1)))
        } catch {
            return .failed(.thrown(error, "New MIDI Destination Failed"))
        }
        return .done(ControlCommands.ran(addMidi, doc, m))
    }
}
