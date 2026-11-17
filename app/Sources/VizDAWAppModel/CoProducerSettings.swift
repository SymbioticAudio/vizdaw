import Foundation
import Observation
import VizDAWCore

/// What the Co-Producer switch asks of the app (plan 8a Ruling 10): the
/// live sink opens and closes the Co-Producer socket and writes the off
/// marker; tests pass a fake that does neither.
@MainActor
public protocol CoProducerSink {
    /// Starts listening at `path`: nil when listening, else the reason it
    /// could not.
    func start(path: String) -> String?
    /// Stops: queued calls are answered `shutting_down`, connections drop.
    func stop()
    /// Writes (`true`) or removes (`false`) `<support>/co-producer.off`.
    func markOff(_ off: Bool)
}

/// Settings › Co-Producer (plan 8a Ruling 10): the switch, off unless the
/// composer turned it on, a preference of this Mac. On, the app listens on
/// `<support>/co-producer.sock` for `vizdaw-mcp`; off, there is no socket,
/// and `<support>/co-producer.off` tells `vizdaw-mcp` why it cannot connect.
@Observable
@MainActor
public final class CoProducerSettings {
    public static let key = "VizDAW.coProducer"
    public static let socketName = "co-producer.sock"
    /// A request with no Co-Producer call this long ends (Ruling 3).
    public static let idleMs: UInt64 = 300_000
    public static let onSentence = "Co-Producer is on: Claude can reach VizDAW through vizdaw-mcp."
    public static let offSentence = "Co-Producer is off: Claude cannot reach VizDAW."

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let sink: any CoProducerSink
    /// The support folder the socket and the off marker live in.
    public let supportDir: URL
    /// The app ends every open request here when the switch goes off.
    @ObservationIgnored public var didTurnOff: (@MainActor () -> Void)?

    /// Listening. False when the key is absent or not a Bool.
    public private(set) var on = false
    /// "Co-Producer could not start: <reason>", after a failed start.
    public private(set) var problem: String?
    /// The saved copies of the open saved projects, as `readCopies` found.
    public private(set) var copies = CopiesSizeRow(count: 0, bytes: 0)

    /// Reads the switch and, when it is on, starts at once.
    public init(defaults: UserDefaults, supportDir: URL, sink: any CoProducerSink) {
        self.defaults = defaults
        self.supportDir = supportDir
        self.sink = sink
        if defaults.object(forKey: Self.key) as? Bool == true {
            turnOn()
        } else {
            sink.markOff(true)
        }
    }

    /// The socket's path.
    public var socketPath: String { supportDir.appendingPathComponent(Self.socketName).path }

    /// Ruling 10's sentence for the switch as it is.
    public var status: String { on ? Self.onSentence : Self.offSentence }

    /// The switch (Ruling 10): kept, then started or stopped.
    public func setOn(_ want: Bool) {
        if want {
            guard !on else { return }
            turnOn()
        } else {
            problem = nil
            guard on else {
                defaults.set(false, forKey: Self.key)
                return
            }
            sink.stop()
            on = false
            didTurnOff?()
            sink.markOff(true)
            defaults.set(false, forKey: Self.key)
        }
    }

    /// The app quits: the server stops and requests end; the off marker
    /// is left as it is (Ruling 10).
    public func quit() {
        guard on else { return }
        sink.stop()
        on = false
        didTurnOff?()
    }

    private func turnOn() {
        sink.markOff(false)
        if let why = sink.start(path: socketPath) {
            problem = "Co-Producer could not start: \(why)"
            on = false
            sink.markOff(true)
            defaults.set(false, forKey: Self.key)
            return
        }
        problem = nil
        on = true
        defaults.set(true, forKey: Self.key)
    }

    /// Counts the saved copies of `store`'s open saved projects.
    public func readCopies(_ store: DocumentStore) {
        var count: UInt64 = 0
        var bytes: UInt64 = 0
        for m in store.documents.values where !m.needsSaveAs {
            guard let c = try? m.session.coProducerCopiesSize() else { continue }
            count += c.count
            bytes += c.bytes
        }
        copies = CopiesSizeRow(count: count, bytes: bytes)
    }

    /// "Saved copies in open projects: N (X MB)".
    public var copiesLine: String {
        let mb = String(format: "%.1f", Double(copies.bytes) / 1_000_000)
        return "Saved copies in open projects: \(copies.count) (\(mb) MB)"
    }

    /// The support folder as `vizdaw-mcp` finds it (Task 5's
    /// `support_dir_from_env`), so the socket and the screenshots it
    /// reads are where it looks: `VIZDAW_SUPPORT_DIR` when set (nil
    /// unless absolute), else `$HOME/Library/Application Support/VizDAW`
    /// (nil unless `HOME` is absolute).
    public nonisolated static func supportPath(environment: [String: String]) -> String? {
        if let d = environment["VIZDAW_SUPPORT_DIR"] {
            return d.hasPrefix("/") ? d : nil
        }
        guard let home = environment["HOME"], home.hasPrefix("/") else { return nil }
        return URL(fileURLWithPath: home).appendingPathComponent("Library/Application Support/VizDAW").path
    }
}
