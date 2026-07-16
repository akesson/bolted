// The U rows, headless: the ViewModel against a REAL syncd on a temp socket — the probe.rs
// pattern from Swift. Gated on BOLTED_SYNCD (the daemon binary's path; test-os-app.sh sets it
// after cargo build) so a bare `swift test` still passes without the Rust toolchain.
//
// Tests may assert on core-provided constraint values (params from the wire) — the "no
// constraint literals" grep binds Sources/, not Tests/ (the step-03 scoping).

import XCTest

@testable import BoltedSyncCore
@testable import SyncWireKit

final class ViewModelWireTests: XCTestCase {
    private var daemon: Process?
    private var socketPath = ""
    private var vm: SyncViewModel!

    override func setUpWithError() throws {
        guard let syncd = ProcessInfo.processInfo.environment["BOLTED_SYNCD"] else {
            throw XCTSkip("BOLTED_SYNCD not set — run via `mise run test:os:app`")
        }
        socketPath = NSTemporaryDirectory() + "bolted-vm-\(UUID().uuidString.prefix(8)).sock"
        try startDaemon(syncd)
        // Pushes applied inline on the callback queue; tests poll for the effects.
        vm = SyncViewModel(apply: { $0() })
        vm.connect(path: socketPath)
        XCTAssertEqual(vm.connectionState, .connected)
    }

    override func tearDown() {
        vm?.disconnect()
        daemon?.terminate()
        daemon = nil
        try? FileManager.default.removeItem(atPath: socketPath)
    }

    private func startDaemon(_ path: String) throws {
        // A stale socket file from a previous daemon would satisfy the bind-wait below before
        // the NEW daemon binds (the test-sandbox.sh lesson) — remove it first.
        try? FileManager.default.removeItem(atPath: socketPath)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: path)
        p.arguments = ["--socket", socketPath]
        p.standardError = FileHandle.nullDevice
        try p.run()
        daemon = p
        try waitUntil("daemon binds \(socketPath)") {
            FileManager.default.fileExists(atPath: self.socketPath)
        }
    }

    private func waitUntil(
        _ what: String, timeout: TimeInterval = 5, _ cond: () -> Bool
    ) throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if cond() { return }
            Thread.sleep(forTimeInterval: 0.02)
        }
        XCTFail("timed out waiting for: \(what)")
        throw XCTSkip("bailing after timeout")
    }

    /// A second client standing in for "another process" (the E-row driver).
    private func otherClient() throws -> WireConnection {
        try XCTUnwrap(WireConnection.connect(path: socketPath))
    }

    // U1 — the menu-bar surface's contract: external mutation → push tick → fetched canonical.
    func testU1_externalToggleReachesTheMenuSurface() throws {
        XCTAssertEqual(vm.canonical?.paused, false)
        let other = try otherClient()
        XCTAssertEqual(other.request(.togglePaused)?.t, "toggled")
        try waitUntil("canonical flips via tick-then-fetch") { self.vm.canonical?.paused == true }
        other.close()
    }

    // U2 — keyed errors render from wire params; the core's numbers, never the shell's.
    func testU2_invalidEditRendersKeyedErrorWithCoreParams() throws {
        vm.openEditor()
        let long = String(repeating: "x", count: 31)
        vm.focusedField = "label"
        vm.labelBuffer = long
        vm.edit(field: "label", text: long)
        let errs = try XCTUnwrap(vm.draft?.report.errors(for: "label"))
        let tooLong = try XCTUnwrap(errs.first { $0.key == "too_long" })
        XCTAssertEqual(tooLong.paramMap["max"], "30")
        XCTAssertEqual(tooLong.paramMap["actual"], "31")
        XCTAssertEqual(
            ErrorMessages.render(tooLong), "Too long — at most 30 characters (got 31).")
        // The echo rule: the rejected text stays in the focused buffer, not the core's raw.
        XCTAssertEqual(vm.labelBuffer, long)
    }

    // U2 — the echo rule under sanitization: core trims, the focused buffer is untouched,
    // blur adopts the sanitized value.
    func testU2_echoRule_focusedBufferSurvivesUntilBlur() throws {
        vm.openEditor()
        vm.focusedField = "label"
        vm.labelBuffer = "  Padded  "
        vm.edit(field: "label", text: "  Padded  ")
        XCTAssertEqual(vm.labelBuffer, "  Padded  ", "focused buffer must not be rewritten")
        XCTAssertEqual(vm.draft?.label.raw?.asText, "Padded", "core sanitized (trim)")
        vm.blur(field: "label")
        XCTAssertEqual(vm.labelBuffer, "Padded", "blur adopts the core raw")
    }

    // U2 — the async check, client-driven over the wire: required → pending → settled, and the
    // verdict is the client's own filesystem judgement (the capability lives client-side).
    func testU2_folderCheck_beginCompleteAcrossTheWire() throws {
        vm.openEditor()
        let realDir = NSTemporaryDirectory()
        vm.edit(field: "folder", text: realDir)
        XCTAssertTrue(
            vm.draft!.report.ruleKeys.contains("folder_check_required"),
            "a dirty folder demands a fresh check (C16)")
        vm.runFolderCheckIfNeeded()
        XCTAssertFalse(vm.draft!.report.ruleKeys.contains("folder_check_required"))
        XCTAssertFalse(vm.draft!.report.ruleKeys.contains("folder_unreachable"))

        vm.edit(field: "folder", text: "/definitely/not/here-\(UUID().uuidString)")
        vm.runFolderCheckIfNeeded()
        XCTAssertTrue(
            vm.draft!.report.ruleKeys.contains("folder_unreachable"),
            "a failed verdict crosses back as the declared failed key")
    }

    // U3 — live rebase + conflict + resolution, with the "other process" submitting under us.
    func testU3_conflictOverTheWire_keepMineThenSubmit() throws {
        vm.openEditor()
        vm.focusedField = "label"
        vm.labelBuffer = "Mine"
        vm.edit(field: "label", text: "Mine")

        let other = try otherClient()
        let draftId = try XCTUnwrap(other.request(.checkout)?.draft)
        XCTAssertEqual(
            other.request(.trySet(draft: draftId, field: "label", value: .text("Theirs")))?.t,
            "set_outcome")
        XCTAssertEqual(other.request(.submit(draft: draftId))?.t, "submitted")

        try waitUntil("the rebase push lands and the snapshot shows the conflict") {
            self.vm.draft?.label.theirs != nil
        }
        XCTAssertEqual(vm.draft?.label.raw?.asText, "Mine", "mine preserved (conflict ceiling)")
        XCTAssertEqual(vm.draft?.label.theirs?.asText, "Theirs")
        XCTAssertEqual(vm.labelBuffer, "Mine", "focused buffer untouched by the rebase")

        vm.resolve(field: "label", keepMine: true)
        XCTAssertNil(vm.draft?.label.theirs, "resolution clears the conflict")
        vm.submit()
        guard case .submitted = vm.lastSubmit else {
            return XCTFail("keep-mine then submit should succeed, got \(String(describing: vm.lastSubmit))")
        }
        try waitUntil("canonical adopts mine") { self.vm.canonical?.label == "Mine" }
        other.close()
    }

    // U4 — the reconnect story: daemon dies under a dirty editor; the stash is ours; the fresh
    // daemon restores it (H6 as a UI feature, C20 visible end-to-end).
    func testU4_daemonDeathThenReconnectRestoresTheDirtyDraft() throws {
        vm.openEditor()
        vm.edit(field: "label", text: "Survives")
        vm.edit(field: "folder", text: NSTemporaryDirectory())
        vm.runFolderCheckIfNeeded()
        XCTAssertFalse(vm.draft!.report.ruleKeys.contains("folder_check_required"))

        daemon?.interrupt()
        daemon?.terminate()
        try waitUntil("the VM notices the death") { self.vm.connectionState == .disconnected }

        let syncd = ProcessInfo.processInfo.environment["BOLTED_SYNCD"]!
        try startDaemon(syncd)
        vm.connect(path: socketPath)
        XCTAssertEqual(vm.connectionState, .connected)
        XCTAssertTrue(vm.restoredFromStash)
        let restored = try XCTUnwrap(vm.draft, "restore should have produced a draft")
        XCTAssertEqual(restored.label.raw?.asText, "Survives")
        XCTAssertEqual(restored.label.dirty, true)
        XCTAssertEqual(vm.labelBuffer, "Survives")
        XCTAssertTrue(
            restored.report.ruleKeys.contains("folder_check_required"),
            "the pre-death PASSED verdict must not survive the restore (C20)")
    }

    // U5 — the number where the user feels it: keystroke (try_set + snapshot + stash) p50.
    func testU5_keystrokeLatencyWellUnderAFrame() throws {
        vm.openEditor()
        vm.focusedField = "label"
        for i in 0..<300 {
            vm.edit(field: "label", text: "Keystroke-\(i % 20)")
        }
        let (p50, p95) = try XCTUnwrap(vm.keystrokeP50P95Micros)
        print("U5 keystroke-to-state: p50=\(p50)µs p95=\(p95)µs (n=300, incl. continuous stash)")
        // Kill bar 2: one 60 Hz frame. The wire floor is ~45µs; this asserts the whole VM path.
        XCTAssertLessThan(p50, 16_000, "keystroke p50 above one frame — kill 2 territory")
    }
}
