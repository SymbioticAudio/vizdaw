import Foundation
import VizDAWCore

/// The one support folder every Swift integration test shares (plan 6d
/// Task 12 fix round 1): made and handed to `setSupportDir` once per test
/// process, so suites running in parallel never repoint it under one
/// another. No test deletes it or sets another; per-test files go in
/// `subfolder(_:)`.
enum TestSupportDir {
    static let shared: URL = {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("vizdaw-swift-test-\(ProcessInfo.processInfo.processIdentifier)")
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            try setSupportDir(path: url.path)
        } catch {
            fatalError("the test support folder: \(error)")
        }
        return url
    }()

    /// A fresh folder of the test's own inside `shared` (the test may
    /// delete it).
    static func subfolder(_ name: String) throws -> URL {
        let url = shared.appendingPathComponent("\(name)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
