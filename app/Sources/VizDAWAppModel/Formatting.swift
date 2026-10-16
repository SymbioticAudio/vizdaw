import Foundation
import VizDAWCore

/// Pure display helpers.
public enum Formatting {
    /// `m:ss.fff`, with a leading `-` before zero (pickups).
    /// Non-finite input gives `--:--.---`.
    public static func time(_ seconds: Double) -> String {
        guard seconds.isFinite else { return "--:--.---" }
        let totalMs = Int64((abs(seconds) * 1000).rounded())
        let sign = seconds < 0 && totalMs > 0 ? "-" : ""
        let minutes = totalMs / 60_000
        let secs = (totalMs / 1000) % 60
        let ms = totalMs % 1000
        return sign + String(format: "%d:%02d.%03d", minutes, secs, ms)
    }

    /// Bar·beat·unit as Rust formats it (`5.2.480`, `0.4.0` in a pickup,
    /// `-1.1.0` before it); empty input shows `-.-.-`.
    public static func bbu(_ position: String) -> String {
        position.isEmpty ? "-.-.-" : position
    }

    /// Toolbar position: `bbu  m:ss.fff`.
    public static func position(_ t: TransportState) -> String {
        "\(bbu(t.positionBbu))  \(time(t.seconds))"
    }

    /// `48 kHz`, `44.1 kHz`; empty for 0.
    public static func sampleRate(_ hz: Double) -> String {
        guard hz > 0, hz.isFinite else { return "" }
        let k = hz / 1000
        let text = k.rounded() == k ? String(format: "%.0f", k) : String(format: "%.1f", k)
        return "\(text) kHz"
    }

    /// `Device · 48 kHz`, or `No output` before the first Play.
    public static func device(_ t: TransportState) -> String {
        t.device.isEmpty ? "No output" : "\(t.device) · \(sampleRate(t.sampleRate))"
    }

    /// The window title: `title`, plus ` — Edited` when dirty.
    public static func windowTitle(_ s: DocSnapshot) -> String {
        s.dirty ? "\(s.title) — Edited" : s.title
    }
}
