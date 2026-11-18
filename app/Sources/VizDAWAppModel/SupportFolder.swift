import Foundation

/// The support folder's owner and mode (plan 8a Ruling 10): Co-Producer's
/// socket folder and time counting's `Metrics/` parent (plan 8f Ruling 1)
/// must be this user's, mode 0700.
public enum SupportFolder {
    /// Makes `dir` (creating it) mode 0700 when this user owns it; else the
    /// reason (8a Ruling 10's sentences).
    @MainActor public static func makePrivate(_ dir: URL) -> String? {
        let fm = FileManager.default
        let path = dir.path
        do {
            try fm.createDirectory(atPath: path, withIntermediateDirectories: true)
            let owner = (try fm.attributesOfItem(atPath: path)[.ownerAccountID] as? NSNumber)?.uint32Value
            guard owner == getuid() else { return "the folder \(path) belongs to another user" }
            try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: path)
        } catch {
            return AlertItem(error, title: "Co-Producer").message
        }
        return nil
    }
}
