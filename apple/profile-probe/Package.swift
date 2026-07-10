// swift-tools-version:5.9
import PackageDescription

// The BoltFFI due-diligence probe (step 02; repointed at the GENERATED bindings in step 11). A
// `swift test`-only package (no app target) that drives the generated bindings from
// `crates/gen-profile-ffi/dist/apple`. Lives under `apple/`, OUTSIDE the cargo workspace, so
// `mise run check` stays Xcode-free.
//
// The dependency is a local path package; its SwiftPM identity is the directory basename
// (`apple`), so product references use `package: "apple"` even though the module is
// `GenProfileFfi`.
let package = Package(
    name: "profile-probe",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "../../crates/gen-profile-ffi/dist/apple"),
    ],
    targets: [
        .testTarget(
            name: "ProfileProbeTests",
            dependencies: [
                .product(name: "GenProfileFfi", package: "apple"),
            ]
        ),
    ]
)
