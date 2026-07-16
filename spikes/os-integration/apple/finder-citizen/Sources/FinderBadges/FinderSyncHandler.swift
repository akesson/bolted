// The FIFinderSync principal class — named by NSExtensionPrincipalClass in the appex
// Info.plist (module-qualified: FinderBadges.FinderSyncHandler).
//
// M0/M1 shape: on load, prove the OS-spawned verdict (G3) — connect to the group socket and
// ping, logging the outcome where the probe script can read it. Badges and the context-menu
// command land in M4.

import BoltedSyncCore
import FinderSync
import SyncWireKit

class FinderSyncHandler: FIFinderSync {
    override init() {
        super.init()
        // Watch nothing yet (M4 re-points this at the canonical folder).
        FIFinderSyncController.default().directoryURLs = []
        Probe.log("spawned pid=\(ProcessInfo.processInfo.processIdentifier)")
        // G2 evidence: what container did the OS actually give this process?
        Probe.log("home=\(NSHomeDirectory())")

        // G3: the OS-spawned, OS-sandboxed process reaches the daemon — or records precisely
        // why not. The connect happens on init so the verdict needs no Finder interaction
        // beyond spawning us.
        switch GroupSocket.connect() {
        case .connected(let client, let path):
            let pong = client.request(.ping)
            Probe.log("G3 connect-ok path=\(path) ping=\(pong?.t ?? "NO-RESPONSE")")
        case .noGroup:
            Probe.log("G3 no-group")
        case .noContainer(let group):
            Probe.log("G3 no-container group=\(group)")
        case .refused(let path, let errno, let message):
            Probe.log("G3 connect-refused path=\(path) errno=\(errno) \(message)")
        }

        // The G3 control (mandatory before G3 counts — the step-10/18 lesson): a socket
        // OUTSIDE the group container must be refused, proving the extension sandbox is on.
        // The path is planted by the probe script via the control file's default location.
        let controlPath = "/tmp/bolted-g3-control.sock"
        switch LineClient.connect(path: controlPath) {
        case .connected:
            Probe.log("G3-CONTROL connect-ok (sandbox NOT proven)")
        case .failed(let errno, let message):
            Probe.log("G3-CONTROL connect-refused errno=\(errno) \(message)")
        }
    }
}

/// Extension processes have no stdout anyone reads; the probe script reads a log file in the
/// group container (writable by both the sandboxed appex and the unsandboxed script).
enum Probe {
    static func log(_ line: String) {
        NSLog("FinderBadges: %@", line)
        guard let group = GroupSocket.groupId(),
            let dir = FileManager.default.containerURL(
                forSecurityApplicationGroupIdentifier: group)
        else { return }
        let url = dir.appendingPathComponent("finder-badges.log")
        let stamped = "\(ProcessInfo.processInfo.processIdentifier) \(line)\n"
        if let handle = try? FileHandle(forWritingTo: url) {
            handle.seekToEndOfFile()
            handle.write(stamped.data(using: .utf8)!)
            try? handle.close()
        } else {
            try? stamped.data(using: .utf8)!.write(to: url)
        }
    }
}
