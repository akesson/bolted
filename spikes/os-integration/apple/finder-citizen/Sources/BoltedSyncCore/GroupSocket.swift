// Where the daemon lives: the step-18 app-group container, one socket path for every surface.
// The group id is not hard-coded (the team prefix differs per maintainer keychain): bundled
// processes read it from Info.plist (the assembly script stamps `BoltedAppGroup`); dev-tier
// unbundled runs take BOLTED_GROUP from the environment.

import Foundation
import SyncWireKit

public enum GroupSocket {
    public static let socketName = "syncd.sock"
    public static let infoPlistKey = "BoltedAppGroup"

    /// The app-group id this process should use, or nil if neither source provides one.
    public static func groupId() -> String? {
        if let env = ProcessInfo.processInfo.environment["BOLTED_GROUP"], !env.isEmpty {
            return env
        }
        return Bundle.main.object(forInfoDictionaryKey: infoPlistKey) as? String
    }

    /// The socket path inside the group container. `containerURL` is the one API that resolves
    /// identically for sandboxed (appex) and unsandboxed (menu-bar app, daemon-side script)
    /// processes — which container it returns *under the sandbox* is probe row G3's terrain.
    public static func socketPath(group: String) -> String? {
        FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: group)?
            .appendingPathComponent(socketName).path
    }

    /// Resolve and connect in one move; every surface starts here.
    public static func connect() -> ConnectResult {
        guard let group = groupId() else { return .noGroup }
        guard let path = socketPath(group: group) else { return .noContainer(group: group) }
        switch SyncWireKit.LineClient.connect(path: path) {
        case .connected(let client): return .connected(client: client, path: path)
        case .failed(let errno, let message):
            return .refused(path: path, errno: errno, message: message)
        }
    }

    public enum ConnectResult {
        case connected(client: LineClient, path: String)
        case noGroup
        case noContainer(group: String)
        case refused(path: String, errno: Int32, message: String)
    }
}
