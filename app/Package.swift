// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "VizDAW",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "VizDAW", targets: ["VizDAW"])
    ],
    targets: [
        // Generated header + module map (scripts/build-ffi.sh).
        .target(name: "vizdaw_ffiFFI", path: "Sources/vizdaw_ffiFFI"),
        // Generated Swift bindings; Swift 5 mode (not Swift 6 clean).
        .target(
            name: "VizDAWCore",
            dependencies: ["vizdaw_ffiFFI"],
            path: "Sources/VizDAWCore",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L\(Context.packageDirectory)/../target/ffi"]),
                .linkedLibrary("vizdaw_ffi"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                // Plugin editors: AppKit views and AU generic views (the
                // static library's own link requests do not reach SwiftPM).
                .linkedFramework("AppKit"),
                .linkedFramework("CoreAudioKit"),
                .linkedFramework("CoreMIDI"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Foundation"),
            ]
        ),
        .target(
            name: "VizDAWAppModel",
            dependencies: ["VizDAWCore"],
            path: "Sources/VizDAWAppModel"
        ),
        // Editors (plan 4a): pure geometry plus the AppKit canvas views.
        .target(
            name: "VizDAWEditors",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Sources/VizDAWEditors"
        ),
        .executableTarget(
            name: "VizDAW",
            dependencies: ["VizDAWAppModel", "VizDAWCore", "VizDAWEditors"],
            path: "Sources/VizDAW"
        ),
        .testTarget(
            name: "VizDAWAppModelTests",
            // VizDAWEditors: arrangement view tests reuse this target's FakeSession.
            dependencies: ["VizDAWAppModel", "VizDAWCore", "VizDAWEditors"],
            path: "Tests/VizDAWAppModelTests"
        ),
        .testTarget(
            name: "VizDAWEditorsTests",
            dependencies: ["VizDAWEditors", "VizDAWAppModel", "VizDAWCore"],
            path: "Tests/VizDAWEditorsTests"
        ),
    ]
)
