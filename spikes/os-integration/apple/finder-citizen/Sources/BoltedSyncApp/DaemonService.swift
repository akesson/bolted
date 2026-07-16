// SMAppService — the app owning its daemon's lifecycle (probe rows S1–S4). Registration is
// bundle-relative: the plist lives in Contents/Library/LaunchAgents/, the daemon binary in
// Contents/MacOS/, and launchd learns both from the running app's own bundle.
//
// Also the probe's CLI: `BoltedSyncApp --daemon status|register|unregister` prints greppable
// lines and exits, so test-os-app.sh can drive the ceremony without a GUI.

import Foundation
import ServiceManagement

enum DaemonService {
    static let plistName = "dev.bolted.sync.daemon.plist"

    static var service: SMAppService {
        SMAppService.agent(plistName: plistName)
    }

    static var statusString: String {
        switch service.status {
        case .notRegistered: return "not_registered"
        case .enabled: return "enabled"
        case .requiresApproval: return "requires_approval"
        case .notFound: return "not_found"
        @unknown default: return "unknown(\(service.status.rawValue))"
        }
    }

    /// The probe CLI. Returns an exit code; every outcome (including the error text of a
    /// refusal) is printed verbatim — the ceremony is the evidence.
    static func runCommand(_ verb: String) -> Int32 {
        switch verb {
        case "status":
            print("daemon-status=\(statusString)")
            return 0
        case "register":
            do {
                try service.register()
                print("daemon-register=ok daemon-status=\(statusString)")
                return 0
            } catch {
                print("daemon-register=failed daemon-status=\(statusString) error=\(error)")
                return 1
            }
        case "unregister":
            do {
                try service.unregister()
                print("daemon-unregister=ok daemon-status=\(statusString)")
                return 0
            } catch {
                print("daemon-unregister=failed daemon-status=\(statusString) error=\(error)")
                return 1
            }
        default:
            FileHandle.standardError.write(
                "usage: BoltedSyncApp --daemon status|register|unregister\n".data(using: .utf8)!)
            return 64
        }
    }
}
