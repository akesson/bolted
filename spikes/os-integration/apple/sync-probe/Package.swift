// swift-tools-version:5.10
// The Swift probe client (step 18, M3/M5): decodes the sync-wire envelope with Codable and a
// POSIX unix socket — no Network.framework, no async runtime, and (the point) no bolted linkage.
import PackageDescription

let package = Package(
    name: "sync-probe",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "SyncProbe",
            path: "Sources/SyncProbe",
            linkerSettings: [
                // Embed a bundle identity: App Sandbox SIGTRAPs at libsecinit for a bare
                // executable without one (M3 finding — recorded in the step-18 report).
                .unsafeFlags([
                    "-Xlinker", "-sectcreate",
                    "-Xlinker", "__TEXT",
                    "-Xlinker", "__info_plist",
                    "-Xlinker", "Resources/Info.plist",
                ])
            ]
        )
    ]
)
