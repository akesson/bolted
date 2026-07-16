// The probe's canonical-change driver: `BoltedSyncApp --drive toggle | set-folder <path>`.
// Runs the same full contract cycle the GUI does (checkout → set → check → submit), through
// the same wire — so G4's "the badge follows canonical" is driven by a real product path, not
// a test backdoor.

import BoltedSyncCore
import Foundation
import SyncWireKit

enum DriveCLI {
    static func run(_ args: [String]) -> Int32 {
        guard let group = GroupSocket.groupId(),
            let path = GroupSocket.socketPath(group: group),
            let conn = WireConnection.connect(path: path)
        else {
            FileHandle.standardError.write("drive: cannot reach the daemon\n".data(using: .utf8)!)
            return 1
        }
        switch args.first {
        case "toggle":
            guard let resp = conn.request(.togglePaused) else { return fail("toggle") }
            print("drive-toggle=\(resp.t) paused=\(resp.paused.map(String.init) ?? "?")")
            return 0
        case "set-folder":
            guard args.count == 2 else { return usage() }
            return setFolder(conn, to: args[1])
        default:
            return usage()
        }
    }

    private static func setFolder(_ conn: WireConnection, to folder: String) -> Int32 {
        guard let draft = conn.request(.checkout)?.draft else { return fail("checkout") }
        guard conn.request(.trySet(draft: draft, field: "folder", value: .text(folder)))?.t
            == "set_outcome"
        else { return fail("try_set") }
        // A dirty folder demands a fresh check before submit (C16) — the driver judges with
        // its own filesystem access, like every other client.
        guard let token = conn.request(.beginCheck(draft: draft, check: "folder_reachable"))?.token
        else { return fail("begin_check") }
        var isDir: ObjCBool = false
        let ok = FileManager.default.fileExists(atPath: folder, isDirectory: &isDir) && isDir.boolValue
        _ = conn.request(
            .completeCheck(draft: draft, check: "folder_reachable", token: token, ok: ok))
        guard let resp = conn.request(.submit(draft: draft)) else { return fail("submit") }
        print("drive-set-folder=\(resp.t) v=\(resp.version.map(String.init) ?? "?")")
        return resp.t == "submitted" ? 0 : 1
    }

    private static func fail(_ what: String) -> Int32 {
        FileHandle.standardError.write("drive: \(what) failed\n".data(using: .utf8)!)
        return 1
    }

    private static func usage() -> Int32 {
        FileHandle.standardError.write(
            "usage: BoltedSyncApp --drive toggle | set-folder <path>\n".data(using: .utf8)!)
        return 64
    }
}
