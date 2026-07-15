// swift-tools-version:5.9
// Consumer of the packed artifact: depends on ../package as ONE dependency —
// this is the "importable as a single dependency" check from architecture.md §4.1.
import PackageDescription

let package = Package(
    name: "SpikeHttpConsumer",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "../package")
    ],
    targets: [
        .testTarget(
            name: "SpikeHttpConsumerTests",
            dependencies: [.product(name: "SpikeHttpFfi", package: "package")],
            path: "Tests/SpikeHttpConsumerTests"
        )
    ]
)
