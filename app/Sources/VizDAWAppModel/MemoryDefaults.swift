import Foundation

/// An in-memory `UserDefaults`: nothing reaches `~/Library/Preferences`.
/// A run with `VIZDAW_SUPPORT_DIR` set (vizctl launch, tests) uses it so a
/// throwaway home also isolates preferences: cfprefsd resolves the real
/// user's plist whatever `$HOME` says.
public final class MemoryDefaults: UserDefaults, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: Any] = [:]

    public init() {
        super.init(suiteName: "VizDAW.memory.\(UUID().uuidString)")!
    }

    /// The preferences store for this process: in memory when
    /// `VIZDAW_SUPPORT_DIR` is set, else `.standard`.
    public static func appStore(environment: [String: String] = ProcessInfo.processInfo.environment)
        -> UserDefaults
    {
        environment["VIZDAW_SUPPORT_DIR"] == nil ? .standard : MemoryDefaults()
    }

    private func get(_ key: String) -> Any? {
        lock.lock()
        defer { lock.unlock() }
        return values[key]
    }

    public override func object(forKey defaultName: String) -> Any? { get(defaultName) }

    public override func set(_ value: Any?, forKey defaultName: String) {
        lock.lock()
        defer { lock.unlock() }
        values[defaultName] = value
    }

    public override func removeObject(forKey defaultName: String) {
        lock.lock()
        defer { lock.unlock() }
        values[defaultName] = nil
    }

    public override func data(forKey defaultName: String) -> Data? { get(defaultName) as? Data }
    public override func string(forKey defaultName: String) -> String? { get(defaultName) as? String }
    public override func array(forKey defaultName: String) -> [Any]? { get(defaultName) as? [Any] }
    public override func dictionary(forKey defaultName: String) -> [String: Any]? {
        get(defaultName) as? [String: Any]
    }
    public override func bool(forKey defaultName: String) -> Bool { get(defaultName) as? Bool ?? false }
    public override func integer(forKey defaultName: String) -> Int { get(defaultName) as? Int ?? 0 }
    public override func double(forKey defaultName: String) -> Double { get(defaultName) as? Double ?? 0 }
    public override func set(_ value: Bool, forKey defaultName: String) { set(value as Any?, forKey: defaultName) }
    public override func set(_ value: Int, forKey defaultName: String) { set(value as Any?, forKey: defaultName) }
    public override func set(_ value: Double, forKey defaultName: String) { set(value as Any?, forKey: defaultName) }
}
