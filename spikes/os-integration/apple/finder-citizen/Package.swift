// swift-tools-version:5.10
// Step 19 — the Finder-citizen app: a menu-bar surface + a FinderSync extension, both riding the
// step-18 wire. SwiftPM only (no Xcode project); the .app/.appex bundles are hand-assembled by
// scripts/assemble-app.sh — the assembly script is itself a probe artifact (recon R1: what would
// `bolted new` scaffolding have to emit?).
import PackageDescription

let package = Package(
    name: "finder-citizen",
    platforms: [.macOS(.v14)],
    targets: [
        // The Codable wire mirror + line client, copied from sync-probe and extended with the
        // verbs a UI needs (resolve / close / stash / restore / stats) — the probe package stays
        // untouched (recorded deviation: copy over cross-package library churn; disposable code).
        .target(name: "SyncWireKit", path: "Sources/SyncWireKit"),
        // The headless-testable heart: connection management, the ViewModel, the reconnect story.
        .target(
            name: "BoltedSyncCore",
            dependencies: ["SyncWireKit"],
            path: "Sources/BoltedSyncCore"
        ),
        // The @main MenuBarExtra shell — as thin as step 03's executable target.
        .executableTarget(
            name: "BoltedSyncApp",
            dependencies: ["BoltedSyncCore"],
            path: "Sources/BoltedSyncApp"
        ),
        // The FinderSync extension executable. Bundled as FinderBadges.appex by the assembly
        // script; the NSExtension entry-point ceremony is M1 probe terrain.
        .executableTarget(
            name: "FinderBadges",
            dependencies: ["SyncWireKit", "BoltedSyncCore"],
            path: "Sources/FinderBadges",
            linkerSettings: [.linkedFramework("FinderSync")]
        ),
        .testTarget(
            name: "BoltedSyncCoreTests",
            dependencies: ["BoltedSyncCore"],
            path: "Tests/BoltedSyncCoreTests"
        ),
    ]
)
