// swift-tools-version:5.9
import PackageDescription

// Step 10's smoke test: the FFI layer of `gen-profile` is *generated*, and this proves the generated
// bindings compile, link and run. It is deliberately small — the full Swift and Kotlin shells still
// link the hand-written `spike-profile-ffi`, and repointing them is step 11.
//
// Outside the cargo workspace, like every other Swift package here, so `mise run check` stays
// Xcode-free.
let package = Package(
    name: "gen-profile-smoke",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "../../crates/gen-profile-ffi/dist/apple"),
    ],
    targets: [
        .testTarget(
            name: "GenProfileSmokeTests",
            dependencies: [
                .product(name: "GenProfileFfi", package: "apple"),
            ]
        ),
    ]
)
