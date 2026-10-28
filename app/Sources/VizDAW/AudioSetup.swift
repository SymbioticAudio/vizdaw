import Foundation
import VizDAWAppModel
import VizDAWCore

/// The core's process-wide audio setters (plan 5d-2 Ruling 2).
@MainActor
final class LiveAudioSink: AudioSink {
    func softwareMonitoring(_ on: Bool) { setSoftwareMonitoring(on: on) }
    func bufferFrames(_ frames: UInt32) throws { try setIoBufferFrames(frames: frames) }
    func inputDevice(_ name: String?) { setAudioInputDevice(name: name) }
    func latencyTrim(_ frames: Int32) { setRecordLatencyTrim(frames: frames) }
    func stream(cacheMb: UInt32, readAheadSeconds: UInt32) throws {
        try setStreamSettings(cacheMb: cacheMb, readAheadSeconds: readAheadSeconds)
    }
    func defaultMonitor(_ mode: MonitorModeRow) { setDefaultMonitorMode(mode: mode) }
}

/// The audio and recording settings (Settings › Audio and › Recording),
/// in memory under `VIZDAW_SUPPORT_DIR` (vizctl, tests).
@MainActor let audioPreferences = AudioPreferences(defaults: MemoryDefaults.appStore(), sink: LiveAudioSink())
