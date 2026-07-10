// swift-tools-version:5.9
import PackageDescription

// The step-03 SwiftUI spike app. A `swift run`/`swift test` package (no Xcode project) that puts a
// real editing surface on the generated bindings (step 11). Lives under `apple/`, OUTSIDE the cargo workspace,
// so `mise run check` stays Xcode-free. Depends on the generated `dist/apple` package exactly as
// the probe does (its SwiftPM identity is the directory basename `apple`).
//
// Split into a library (the hand-written "as-if-generated" ViewModel + views, headlessly tested)
// and a thin executable (the `@main` App, run for the manual protocol — never in CI).
let package = Package(
    name: "profile-app",
    platforms: [.macOS(.v14)], // @Observable
    products: [
        // Exposed so the XcodeGen UI-test project (apple/profile-app/project.yml) can build an
        // Xcode app target from ProfileApp.swift while consuming the feature library. `swift
        // build`/`swift test` are unaffected — this only adds an externally-visible product.
        .library(name: "ProfileFeature", targets: ["ProfileFeature"]),
    ],
    dependencies: [
        .package(path: "../../crates/gen-profile-ffi/dist/apple"),
    ],
    targets: [
        .target(
            name: "ProfileFeature",
            dependencies: [
                .product(name: "GenProfileFfi", package: "apple"),
            ]
        ),
        .executableTarget(
            name: "ProfileApp",
            dependencies: ["ProfileFeature"]
        ),
        .testTarget(
            name: "ProfileFeatureTests",
            dependencies: ["ProfileFeature"]
        ),
    ]
)
