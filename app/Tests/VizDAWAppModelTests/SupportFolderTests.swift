import Foundation
import Testing
import VizDAWAppModel

/// 8a Ruling 10's owner and 0700 check, shared with time counting (plan 8f
/// Task 4). Temp folders only.
@MainActor
@Suite struct SupportFolderTests {
    private func temp() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-support-\(UUID().uuidString)")
    }

    private func mode(_ url: URL) throws -> Int {
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        return try #require(attrs[.posixPermissions] as? NSNumber).intValue & 0o777
    }

    @Test func aNewFolderIsMadePrivate() throws {
        let dir = temp().appendingPathComponent("VizDAW")
        defer { try? FileManager.default.removeItem(at: dir.deletingLastPathComponent()) }
        #expect(SupportFolder.makePrivate(dir) == nil)
        var isDir: ObjCBool = false
        #expect(FileManager.default.fileExists(atPath: dir.path, isDirectory: &isDir) && isDir.boolValue)
        #expect(try mode(dir) == 0o700)
    }

    @Test func anOpenFolderIsClosedTo0700() throws {
        let dir = temp()
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
        #expect(try mode(dir) == 0o755)
        #expect(SupportFolder.makePrivate(dir) == nil)
        #expect(try mode(dir) == 0o700)
    }

    @Test func aFileIsRefusedWithAReason() throws {
        let file = temp()
        defer { try? FileManager.default.removeItem(at: file) }
        #expect(FileManager.default.createFile(atPath: file.path, contents: Data("x".utf8)))
        let why = SupportFolder.makePrivate(file)
        #expect(why != nil)
        #expect(why?.isEmpty == false)
        // Left as it was.
        #expect(try Data(contentsOf: file) == Data("x".utf8))
    }
}
