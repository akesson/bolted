// The FIFinderSync principal class — named by NSExtensionPrincipalClass in the appex
// Info.plist (module-qualified: FinderBadges.FinderSyncHandler).
//
// M1 (kept, the script greps it): the G3 verdict on init — the OS-spawned, OS-sandboxed
// process connects to the group socket and pings, plus the outside-container control.
// M4: the living citizen — the watched directory IS the canonical `folder`, the badge IS the
// canonical `paused`, both kept current by tick-then-fetch over a WireConnection; the context
// menu issues the session-less command (G5); daemon death is survived by a 2 s reconnect loop,
// which under launchd socket activation quietly RESURRECTS the daemon (the whole topology in
// one gesture).

import BoltedSyncCore
import FinderSync
import SyncWireKit

class FinderSyncHandler: FIFinderSync {
    /// The live wire — pushes drive badge state; requests ride the same connection.
    var wire: WireConnection?
    var canonical: CanonicalW?
    let reconnectQueue = DispatchQueue(label: "dev.bolted.sync.finderbadges.reconnect")

    override init() {
        super.init()
        Probe.log("spawned pid=\(ProcessInfo.processInfo.processIdentifier)")
        Probe.log("home=\(NSHomeDirectory())")

        // Two badges, keyed by the canonical `paused` — state from the wire, art from the shell.
        let controller = FIFinderSyncController.default()
        if let active = NSImage(systemSymbolName: "checkmark.circle.fill", accessibilityDescription: nil) {
            controller.setBadgeImage(active, label: "Synced", forBadgeIdentifier: "active")
        }
        if let paused = NSImage(systemSymbolName: "pause.circle.fill", accessibilityDescription: nil) {
            controller.setBadgeImage(paused, label: "Paused", forBadgeIdentifier: "paused")
        }

        // G3 first (M1's probe rows, LineClient for verbatim errnos), then the live wire.
        probeG3()
        connectLiveWire()
    }

    // ---------------------------------------------------------------------------------------------
    // M1 — the G3 verdict + control (test-os-app.sh greps these lines)
    // ---------------------------------------------------------------------------------------------

    private func probeG3() {
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
        // The probe script keeps a live daemon on this path so EPERM can only be the sandbox.
        switch LineClient.connect(path: "/tmp/bolted-g3-control.sock") {
        case .connected:
            Probe.log("G3-CONTROL connect-ok (sandbox NOT proven)")
        case .failed(let errno, let message):
            Probe.log("G3-CONTROL connect-refused errno=\(errno) \(message)")
        }
    }

    // ---------------------------------------------------------------------------------------------
    // M4 — the live badge state (G4) + reconnect
    // ---------------------------------------------------------------------------------------------

    private func connectLiveWire() {
        guard let group = GroupSocket.groupId(),
            let path = GroupSocket.socketPath(group: group)
        else { return }
        guard let conn = WireConnection.connect(path: path) else {
            Probe.log("live-wire connect failed — retrying in 2s")
            scheduleReconnect()
            return
        }
        // M4c finding: under socket activation, connect(2) success is NOT daemon liveness — a
        // post-crash connect can sit in launchd's listener backlog unaccepted. Verify the
        // session with a round-trip before believing it (open-then-verify; generator evidence).
        guard conn.request(.ping, timeoutSeconds: 5) != nil else {
            Probe.log("live-wire connect unverified (no pong) — retrying in 2s")
            conn.close()
            scheduleReconnect()
            return
        }
        conn.onPush = { [weak self] push in
            guard let self, push.t == "canonical_changed" else { return }
            self.refreshFromCanonical()
        }
        conn.onDisconnect = { [weak self] in
            guard let self else { return }
            Probe.log("live-wire disconnected — retrying in 2s")
            self.wire = nil
            self.scheduleReconnect()
        }
        wire = conn
        Probe.log("live-wire connected")
        refreshFromCanonical()
    }

    /// Under launchd socket activation, this retry loop does more than reconnect: the connect
    /// itself respawns a killed daemon. Surfaces heal the topology by observing it.
    private func scheduleReconnect() {
        reconnectQueue.asyncAfter(deadline: .now() + 2) { [weak self] in
            self?.connectLiveWire()
        }
    }

    private func refreshFromCanonical() {
        guard let resp = wire?.request(.canonicalSnapshot), let c = resp.canonical else { return }
        let before = canonical
        canonical = c
        // Observe-over-wire driving an OS API: the canonical `folder` IS the watched directory.
        let url = URL(fileURLWithPath: c.folder, isDirectory: true)
        FIFinderSyncController.default().directoryURLs = [url]
        Probe.log("watching folder=\(c.folder) paused=\(c.paused) v=\(c.version)")
        // A paused flip re-badges everything already on screen.
        if before?.paused != c.paused {
            for u in badged {
                FIFinderSyncController.default().setBadgeIdentifier(badgeId, for: u)
            }
        }
    }

    private var badged: Set<URL> = []
    private var badgeId: String { canonical?.paused == true ? "paused" : "active" }

    override func requestBadgeIdentifier(for url: URL) {
        badged.insert(url)
        FIFinderSyncController.default().setBadgeIdentifier(badgeId, for: url)
    }

    override func beginObservingDirectory(at url: URL) {
        Probe.log("finder-observing \(url.path)")
    }

    override func endObservingDirectory(at url: URL) {
        badged = badged.filter { !$0.path.hasPrefix(url.path) }
    }

    // ---------------------------------------------------------------------------------------------
    // M4 — the session-less command from Finder's context menu (G5; manual protocol row)
    // ---------------------------------------------------------------------------------------------

    override var toolbarItemName: String { "Bolted Sync" }
    override var toolbarItemToolTip: String { "Bolted Sync badge state" }
    override var toolbarItemImage: NSImage {
        NSImage(systemSymbolName: "arrow.triangle.2.circlepath", accessibilityDescription: nil)
            ?? NSImage()
    }

    override func menu(for menuKind: FIMenuKind) -> NSMenu? {
        let menu = NSMenu(title: "")
        let title = canonical?.paused == true ? "Resume Bolted Sync" : "Pause Bolted Sync"
        let item = NSMenuItem(title: title, action: #selector(togglePaused(_:)), keyEquivalent: "")
        item.target = self
        menu.addItem(item)
        return menu
    }

    @objc private func togglePaused(_ sender: AnyObject?) {
        let resp = wire?.request(.togglePaused)
        Probe.log("G5 context-menu toggle -> \(resp?.t ?? "NO-RESPONSE") paused=\(resp?.paused.map(String.init) ?? "?")")
        // The fan-out tick will re-badge; nothing else to do — the command is one verb.
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
