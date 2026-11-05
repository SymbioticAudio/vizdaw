// swift-tools-version:6.0
import PackageDescription

/// SymbioticAudio's branded glyphs (all rights reserved; linked under the GPL
/// section 7 permission in LICENSE). `VIZDAW_NO_BRAND=1` builds without them
/// (forks), on the standard icon set (plan 7b Ruling 6).
let brand: [Target.Dependency] =
    Context.environment["VIZDAW_NO_BRAND"] == "1"
    ? [] : [.product(name: "SymbioticBrand", package: "symbiotic-design")]
let tokens: Target.Dependency = .product(name: "SymbioticTokens", package: "symbiotic-design")
let icons: Target.Dependency = .product(name: "SymbioticIcons", package: "symbiotic-design")
let controls: Target.Dependency = .product(name: "SymbioticControls", package: "symbiotic-design")

let package = Package(
    name: "VizDAW",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "VizDAW", targets: ["VizDAW"])
    ],
    dependencies: [
        .package(url: "https://github.com/SymbioticAudio/symbiotic-design", exact: "0.2.2")
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
                // The microphone permission (plan 5c Task 11).
                .linkedFramework("AVFoundation"),
            ]
        ),
        .target(
            name: "VizDAWAppModel",
            dependencies: ["VizDAWCore", tokens, icons],
            path: "Sources/VizDAWAppModel"
        ),
        // Editors (plan 4a): pure geometry plus the AppKit canvas views.
        .target(
            name: "VizDAWEditors",
            dependencies: ["VizDAWAppModel", "VizDAWCore", tokens],
            path: "Sources/VizDAWEditors"
        ),
        .executableTarget(
            name: "VizDAW",
            dependencies: ["VizDAWAppModel", "VizDAWCore", "VizDAWEditors", tokens, icons, controls] + brand,
            path: "Sources/VizDAW"
        ),
        .testTarget(
            name: "VizDAWAppModelTests",
            // VizDAWEditors: arrangement view tests reuse this target's FakeSession.
            // No test target links SymbioticBrand: tests run on the standard icons.
            // SymbioticControls: the strip's measurements are pinned to ControlMetrics.
            dependencies: ["VizDAWAppModel", "VizDAWCore", "VizDAWEditors", tokens, icons, controls],
            path: "Tests/VizDAWAppModelTests"
        ),
        .testTarget(
            name: "VizDAWEditorsTests",
            dependencies: ["VizDAWEditors", "VizDAWAppModel", "VizDAWCore", tokens],
            path: "Tests/VizDAWEditorsTests"
        ),
    ]
)
