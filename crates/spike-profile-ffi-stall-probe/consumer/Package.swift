// swift-tools-version:5.9
// Consumer of the packed SpikeProfileFfi package — proves single-dependency consumption
// and hosts the step-02 probe tests.
import PackageDescription

let package = Package(
    name: "SpikeProfileConsumer",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "../package")
    ],
    targets: [
        .testTarget(
            name: "SpikeProfileConsumerTests",
            dependencies: [
                .product(name: "SpikeProfileFfi", package: "package")
            ]
        )
    ]
)
