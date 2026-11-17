import Darwin
import Foundation
import Security
import UniformTypeIdentifiers
import VizDAWAppModel

/// The facts `ClientIdentity.decide` reads about real processes (plan 8b
/// Ruling 11): Security for the code signature (found by audit token for
/// the socket's peer, by pid for its parents), then `proc_pidpath`,
/// `proc_pidinfo(PROC_PIDTBSDINFO)` and `sysctl(KERN_PROCARGS2)`. Nothing
/// here trusts what a process says about itself; anything that cannot be
/// read is absent, which `decide` treats as not verified.
struct DarwinClientFacts: ClientFacts {
    func peer(auditToken: [UInt32]) -> CodeFacts? {
        guard auditToken.count == 8 else { return nil }
        let token = auditToken.withUnsafeBufferPointer { Data(buffer: $0) }
        let attributes = [kSecGuestAttributeAudit: token] as CFDictionary
        var code: SecCode?
        guard SecCodeCopyGuestWithAttributes(nil, attributes, [], &code) == errSecSuccess, let code else {
            return nil
        }
        return facts(pid: Int32(bitPattern: auditToken[5]), code: code)
    }

    func process(pid: Int32) -> CodeFacts? {
        guard pid > 0 else { return nil }
        let attributes = [kSecGuestAttributePid: NSNumber(value: pid)] as CFDictionary
        var code: SecCode?
        guard SecCodeCopyGuestWithAttributes(nil, attributes, [], &code) == errSecSuccess, let code else {
            return nil
        }
        return facts(pid: pid, code: code)
    }

    func bundledMcpHash() -> Data? {
        guard let exe = Bundle.main.executableURL else { return nil }
        let mcp = exe.deletingLastPathComponent().appendingPathComponent("vizdaw-mcp")
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(mcp as CFURL, [], &code) == errSecSuccess, let code,
            SecStaticCodeCheckValidity(code, [], nil) == errSecSuccess
        else { return nil }
        return signing(code)[kSecCodeInfoUnique as String] as? Data
    }

    func bundleName(_ app: URL) -> String? {
        guard let b = Bundle(url: app) else { return nil }
        let name = b.object(forInfoDictionaryKey: "CFBundleDisplayName") ?? b.object(forInfoDictionaryKey: "CFBundleName")
        return (name as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    /// `code`'s signature, and process `pid`'s facts (read with the code
    /// in hand, so the process cannot have been replaced before it).
    private func facts(pid: Int32, code: SecCode) -> CodeFacts? {
        guard let bsd = Self.bsdInfo(pid) else { return nil }
        let valid = SecCodeCheckValidity(code, [], nil) == errSecSuccess
        var staticCode: SecStaticCode?
        let info = SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess ? staticCode.map(signing) ?? [:] : [:]
        let flags = (info[kSecCodeInfoFlags as String] as? NSNumber)?.uint32Value ?? 0
        let entitlements = info[kSecCodeInfoEntitlementsDict as String] as? [String: Any] ?? [:]
        let path = Self.path(pid)
        return CodeFacts(
            pid: pid, parent: Int32(bitPattern: bsd.pbi_ppid),
            started: bsd.pbi_start_tvsec &* 1_000_000 &+ bsd.pbi_start_tvusec, valid: valid,
            team: (info[kSecCodeInfoTeamIdentifier as String] as? String).flatMap { $0.isEmpty ? nil : $0 },
            identifier: info[kSecCodeInfoIdentifier as String] as? String,
            cdhash: info[kSecCodeInfoUnique as String] as? Data,
            hardened: flags & SecCodeSignatureFlags.runtime.rawValue != 0,
            getTaskAllow: entitlements["com.apple.security.get-task-allow"] as? Bool == true,
            path: path, appBundle: path.flatMap(Self.appBundle), launch: Self.launch(pid))
    }

    /// Signing information: the team, identifier, cdhash, flags and
    /// entitlements.
    private func signing(_ code: SecStaticCode) -> [String: Any] {
        var info: CFDictionary?
        let flags = SecCSFlags(rawValue: kSecCSSigningInformation | kSecCSRequirementInformation)
        guard SecCodeCopySigningInformation(code, flags, &info) == errSecSuccess else { return [:] }
        return info as? [String: Any] ?? [:]
    }

    private static func bsdInfo(_ pid: Int32) -> proc_bsdinfo? {
        var info = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size else { return nil }
        return info
    }

    private static func path(_ pid: Int32) -> String? {
        var buffer = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
        let n = proc_pidpath(pid, &buffer, UInt32(buffer.count))
        guard n > 0 else { return nil }
        return String(decoding: buffer.prefix(Int(n)).map { UInt8(bitPattern: $0) }, as: UTF8.self)
    }

    /// The `.app` whose main executable is at `path`, else nil.
    static func appBundle(_ path: String) -> URL? {
        let exe = URL(fileURLWithPath: path)
        let macOS = exe.deletingLastPathComponent()
        let contents = macOS.deletingLastPathComponent()
        let app = contents.deletingLastPathComponent()
        guard macOS.lastPathComponent == "MacOS", contents.lastPathComponent == "Contents",
            UTType(filenameExtension: app.pathExtension)?.conforms(to: .applicationBundle) == true,
            let main = Bundle(url: app)?.executableURL
        else { return nil }
        return main.resolvingSymlinksInPath().path == exe.resolvingSymlinksInPath().path ? app : nil
    }

    /// The arguments and environment (`KERN_PROCARGS2`); empty when they
    /// cannot be read.
    static func launch(_ pid: Int32) -> [String] {
        var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
        var size = 0
        guard sysctl(&mib, 3, nil, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return [] }
        var buffer = [UInt8](repeating: 0, count: size)
        guard sysctl(&mib, 3, &buffer, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return [] }
        return parseProcArgs(Array(buffer.prefix(size)))
    }

    /// `KERN_PROCARGS2`'s layout: argc, the executable path, NUL padding,
    /// then argc arguments and the environment, each NUL-terminated.
    static func parseProcArgs(_ bytes: [UInt8]) -> [String] {
        guard bytes.count > 4 else { return [] }
        let argc = bytes.prefix(4).withUnsafeBytes { $0.loadUnaligned(as: Int32.self) }
        var i = 4
        while i < bytes.count, bytes[i] != 0 { i += 1 }  // the executable path
        while i < bytes.count, bytes[i] == 0 { i += 1 }  // its padding
        var out: [String] = []
        var args = 0
        while i < bytes.count {
            let start = i
            while i < bytes.count, bytes[i] != 0 { i += 1 }
            if i == start {
                // An empty string ends the environment.
                if args >= argc { break }
            }
            out.append(String(decoding: bytes[start..<i], as: UTF8.self))
            args += 1
            i += 1
        }
        return out
    }
}
