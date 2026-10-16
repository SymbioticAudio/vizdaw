// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "VizDAW",
    platforms: [.macOS(.v14)],
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
        .testTarget(
            name: "VizDAWAppModelTests",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Tests/VizDAWAppModelTests"
        ),
    ]
)
