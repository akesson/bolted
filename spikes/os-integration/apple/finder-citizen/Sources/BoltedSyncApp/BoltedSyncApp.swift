// The menu-bar shell — as thin as step 03's executable target. All behavior lives in
// BoltedSyncCore where it is headless-testable; this file is scene layout only.

import BoltedSyncCore
import SwiftUI

@main
struct BoltedSyncApp: App {
    var body: some Scene {
        MenuBarExtra("Bolted Sync", systemImage: "arrow.triangle.2.circlepath") {
            // M0 walking skeleton: prove the target shape (MenuBarExtra from a bare SPM
            // executable — recon R5). The live surface arrives in M2.
            switch GroupSocket.connect() {
            case .connected(_, let path):
                Text("Daemon reachable at \(path)")
            case .noGroup:
                Text("No app group configured (BOLTED_GROUP / Info.plist)")
            case .noContainer(let group):
                Text("No container for group \(group)")
            case .refused(_, let errno, let message):
                Text("Daemon unreachable: errno=\(errno) \(message)")
            }
        }
    }
}
