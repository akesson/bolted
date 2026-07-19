// swift-tools-version:5.9
import PackageDescription

// The conformance test tier for the Apple HTTP adapter (step 25 M0). A `swift test`-only package
// that depends on the packed, bundled adapter package at `../bolted-http` (generated Package.swift +
// xcframework + the hand-written `BoltedHttp.swift`) as ONE path dependency — the "importable as a
// single dependency" shape from spike-packaging-report §1.
//
// Why a SEPARATE package (not a test target inside `apple/bolted-http`): the bundled layout
// *regenerates* `apple/bolted-http/Package.swift` on every `boltffi pack`, so a hand-added test
// target there would be clobbered. The packaging report's own open question resolves exactly this
// way — consumers depend on the packed package rather than packing into their own.
//
// The dependency's SwiftPM identity is its directory basename (`bolted-http`), so the product is
// referenced as `package: "bolted-http"` even though the module is `BoltedHttpApple`.
let package = Package(
    name: "bolted-http-conformance",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "../bolted-http"),
    ],
    targets: [
        .testTarget(
            name: "BoltedHttpConformanceTests",
            dependencies: [
                .product(name: "BoltedHttpApple", package: "bolted-http"),
            ]
        ),
    ]
)
