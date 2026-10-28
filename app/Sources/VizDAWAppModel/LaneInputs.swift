import Foundation
import Observation
import VizDAWCore

/// One entry of a lane's input picker.
public struct InputChoice: Equatable, Sendable {
    public let title: String
    public let input: LaneInputRow?

    public init(_ title: String, _ input: LaneInputRow?) {
        self.title = title
        self.input = input
    }
}

/// A titled section of the input picker ("" for the first, untitled one).
public struct InputGroup: Equatable, Sendable {
    public let title: String
    public let choices: [InputChoice]
}

/// The lane header's and the mixer's input controls (spec §6.9; plan 5d-2
/// Task 4): the input picker (audio channels by the device's names, or a
/// MIDI source and channel), the monitor button that cycles A / I / –, and
/// the input meter with its clip mark. Choices are lane edits (one undo
/// step each); while recording the core refuses input changes (`Busy`, plan
/// 5d-1 Ruling 18) and the status line says so.
@Observable
@MainActor
public final class LaneInputs {
    @ObservationIgnored weak var document: DocumentModel?
    /// The device's input as last read (when a picker opens).
    public private(set) var info: InputInfoRow?
    /// Each metered lane's level (0…1, peaks falling back slowly).
    public private(set) var levels: [UInt64: Float] = [:]
    /// Lanes whose clip mark is lit.
    public private(set) var clipped: Set<UInt64> = []
    @ObservationIgnored private var clipUntil: [UInt64: ContinuousClock.Instant] = [:]
    @ObservationIgnored private var clippedCounts: [UInt64: UInt64] = [:]
    @ObservationIgnored private var destNames: (version: UInt64, names: [UInt64: String])?
    /// The clock the clip hold runs on (tests replace it).
    @ObservationIgnored var now: () -> ContinuousClock.Instant = { ContinuousClock.now }

    /// The MIDI sources the picker offers (tests replace it).
    public static var midiPorts: () -> [String] = { midiSourcePorts() }
    /// How long a clip mark stays lit (plan 5d-1 Ruling 27).
    public static let clipHold: Duration = .seconds(2)

    init(document: DocumentModel) { self.document = document }

    // MARK: The picker

    /// Reads the device's channels and names again (a picker opens).
    public func refreshInfo() {
        guard let m = document else { return }
        if let i = try? m.session.inputInfo() { info = i }
    }

    /// Whether the lane records audio (it plays to an audio path or has an
    /// audio input) or MIDI; a new lane with neither offers both.
    nonisolated static func kinds(_ lane: LaneRow) -> (audio: Bool, midi: Bool) {
        switch lane.input {
        case .audio: return (true, false)
        case .midi: return (false, true)
        case nil:
            let audio = lane.audioOut != nil
            let midi = lane.midiDest != nil
            return audio == midi ? (true, true) : (audio, midi)
        }
    }

    /// The picker's sections for `lane`.
    public func groups(for lane: LaneRow) -> [InputGroup] {
        var out = [InputGroup(title: "", choices: [InputChoice("No Input", nil)])]
        let kinds = Self.kinds(lane)
        if kinds.audio {
            var returnOf: UInt64?
            if case .audio(_, _, let r) = lane.input { returnOf = r }
            let n = max(Int(info?.channels ?? 0), 2)
            var choices: [InputChoice] = []
            for c in 0..<n {
                choices.append(InputChoice(channelTitle(c), .audio(first: UInt16(c), stereo: false, returnOf: returnOf)))
            }
            for c in stride(from: 0, to: n - 1, by: 2) {
                choices.append(InputChoice("In \(c + 1)+\(c + 2)", .audio(first: UInt16(c), stereo: true, returnOf: returnOf)))
            }
            out.append(InputGroup(title: "Audio", choices: choices))
        }
        if kinds.midi {
            var port: String?
            var channel: UInt8?
            if case .midi(let p, let c) = lane.input {
                port = p
                channel = c
            }
            let ports = [InputChoice("All MIDI Inputs", .midi(port: nil, channel: channel))]
                + Self.midiPorts().map { InputChoice($0, .midi(port: $0, channel: channel)) }
            out.append(InputGroup(title: "MIDI Input", choices: ports))
            let channels = [InputChoice("Any Channel", .midi(port: port, channel: nil))]
                + (0..<16).map { InputChoice("Channel \($0 + 1)", .midi(port: port, channel: UInt8($0))) }
            out.append(InputGroup(title: "MIDI Channel", choices: channels))
        }
        return out
    }

    private func channelTitle(_ c: Int) -> String {
        let name = info?.names.indices.contains(c) == true ? info?.names[c] ?? "" : ""
        return name.isEmpty ? "In \(c + 1)" : "In \(c + 1) (\(name))"
    }

    /// What the picker shows for the lane's input now.
    nonisolated public static func title(_ input: LaneInputRow?) -> String {
        switch input {
        case nil: return "No Input"
        case .audio(let first, let stereo, _):
            let a = Int(first) + 1
            return stereo ? "In \(a)+\(a + 1)" : "In \(a)"
        case .midi(let port, let channel):
            let p = port ?? "All MIDI"
            return channel.map { "\(p) · Ch \(Int($0) + 1)" } ?? p
        }
    }

    /// The link badge of a lane that records a synth's return (spec §6.9):
    /// "⇠ ‹synth›"; nil for other lanes.
    public func badge(_ lane: LaneRow) -> String? {
        guard case .audio(_, _, let r?) = lane.input else { return nil }
        return "⇠ " + (destName(r) ?? "Synth")
    }

    private func destName(_ id: UInt64) -> String? {
        guard let m = document else { return nil }
        let v = m.snapshot.version
        if destNames?.version != v {
            let rows = (try? m.session.destinations()) ?? []
            destNames = (v, Dictionary(rows.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a }))
        }
        return destNames?.names[id]
    }

    /// Sets the lane's input (one undo step "Set Input").
    public func choose(_ choice: InputChoice, lane: LaneRow) {
        guard let m = document, let seq = m.selectedSeq, choice.input != lane.input else { return }
        m.editor.edit(.lane(edit: .setInput(seq: seq, lane: lane.id, input: choice.input)))
    }

    // MARK: The monitor button

    /// Auto → Input → Off → Auto.
    nonisolated public static func next(_ mode: MonitorModeRow) -> MonitorModeRow {
        switch mode {
        case .auto: .input
        case .input: .off
        case .off: .auto
        }
    }

    /// The button's letter: A, I or –.
    nonisolated public static func label(_ mode: MonitorModeRow) -> String {
        switch mode {
        case .auto: "A"
        case .input: "I"
        case .off: "–"
        }
    }

    /// The button's help and VoiceOver text.
    nonisolated public static func help(_ mode: MonitorModeRow) -> String {
        switch mode {
        case .auto: "Input monitoring: Auto (heard while armed and stopped, or recording in range)"
        case .input: "Input monitoring: always the input"
        case .off: "Input monitoring: off"
        }
    }

    /// The monitor button: the next mode (one undo step).
    public func cycleMonitor(_ lane: LaneRow) {
        guard let m = document, let seq = m.selectedSeq else { return }
        m.editor.edit(.lane(edit: .setMonitor(seq: seq, lane: lane.id, monitor: Self.next(lane.monitor))))
    }

    // MARK: The meter

    /// Software Monitoring as the core has it (tests replace it).
    public static var softwareMonitoring: () -> Bool = { audioSettings().softwareMonitoring }
    /// Software Monitoring as the last meter poll read it.
    public private(set) var monitoringOn = true
    /// The input is opening (or waiting on macOS's microphone prompt) while
    /// a lane shows a meter (plan 5d-2 pre-flight I5).
    public private(set) var opening = false
    /// The status line while the input opens.
    public static let openingNote = "Opening the audio input… If macOS asks for microphone access, answer its prompt."

    /// The input's peaks since the meters' last read, and since the control
    /// channel's (`state.get input`): the core's peaks are read and reset,
    /// so one read feeds both. Each reader sees every peak since its own
    /// last read (5d-1 carry #1).
    @ObservationIgnored private var meterHeld: [Float] = []
    @ObservationIgnored private var channelHeld: [Float] = []

    /// A lane shows its input meter while it is armed or monitors its
    /// audio input (spec §6.9).
    nonisolated public static func showsMeter(_ lane: LaneRow, softwareMonitoring: Bool) -> Bool {
        guard case .audio = lane.input else { return false }
        return lane.arm || (softwareMonitoring && lane.monitor == .input)
    }

    /// Spec §6.9 with plan 5d-1 M18: an Input-monitored lane meters only while Software Monitoring is on.
    public func showsMeter(_ lane: LaneRow) -> Bool { Self.showsMeter(lane, softwareMonitoring: monitoringOn) }

    /// Some shown lane meters its input: the window polls faster.
    public var metering: Bool { (document?.lanes ?? []).contains { showsMeter($0) } }

    public func level(_ lane: UInt64) -> Float { levels[lane] ?? 0 }

    public func isClipped(_ lane: UInt64) -> Bool { clipped.contains(lane) }

    /// Reads the core's peaks once (they reset) into both readers' holds.
    private func readPeaks() {
        guard let m = document, let raw = try? m.session.inputLevels() else { return }
        meterHeld = Self.merged(meterHeld, raw)
        channelHeld = Self.merged(channelHeld, raw)
    }

    /// Elementwise louder of the two, sanitized; as long as the longer.
    nonisolated static func merged(_ a: [Float], _ b: [Float]) -> [Float] {
        (0..<max(a.count, b.count)).map { i in
            max(
                a.indices.contains(i) ? MixerMath.sanitizedPeak(a[i]) : 0,
                b.indices.contains(i) ? MixerMath.sanitizedPeak(b[i]) : 0)
        }
    }

    /// The input's peaks per channel since the control channel's last read
    /// (`state.get input`); the meters' reads do not take them.
    public func takeChannelLevels() -> [Float] {
        readPeaks()
        let out = channelHeld
        channelHeld = []
        return out
    }

    private func setOpening(_ on: Bool, _ m: DocumentModel) {
        guard on != opening else { return }
        opening = on
        if on {
            m.editor.status = Self.openingNote
        } else if m.editor.status == Self.openingNote {
            m.editor.status = nil
        }
    }

    /// Reads the input's peaks since the last read (the poll calls this
    /// while metering): each metered lane takes the louder of its channels;
    /// a peak at full scale, or a recording's new clipped samples, lights
    /// the clip mark for two seconds. While metering it also notes an
    /// input that is still opening.
    public func pollMeters() {
        let on = Self.softwareMonitoring()
        if on != monitoringOn { monitoringOn = on }
        guard let m = document else { return }
        // Clip counts are per recording (pre-flight M6).
        if !m.record.isRecording { clippedCounts = [:] }
        guard metering else {
            meterHeld = []
            setOpening(false, m)
            if !levels.isEmpty { levels = [:] }
            if !clipped.isEmpty { clipped = [] }
            return
        }
        setOpening((try? m.session.inputInfo())?.opening ?? false, m)
        readPeaks()
        let raw = meterHeld
        meterHeld = []
        let t = now()
        var next: [UInt64: Float] = [:]
        for lane in m.lanes where showsMeter(lane) {
            guard case .audio(let first, let stereo, _) = lane.input else { continue }
            let f = Int(first)
            let chans = stereo ? [f, f + 1] : [f]
            let peak = chans.compactMap { raw.indices.contains($0) ? raw[$0] : nil }.max() ?? 0
            next[lane.id] = max(peak, level(lane.id) * MixerMath.meterDecay)
            let count = m.record.state?.lanes.first { $0.lane == lane.id }?.clipped ?? 0
            if peak >= 1 || count > (clippedCounts[lane.id] ?? 0) { clipUntil[lane.id] = t + Self.clipHold }
            clippedCounts[lane.id] = count
        }
        if next != levels { levels = next }
        clipUntil = clipUntil.filter { $0.value > t && next[$0.key] != nil }
        let lit = Set(clipUntil.keys)
        if lit != clipped { clipped = lit }
    }
}
