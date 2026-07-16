// The step-03 ViewModel patterns, re-run with the core on the other side of a Unix socket:
// echo-rule buffers, client-driven async check, conflict resolution from snapshot data, submit
// rendering returned reports — plus the piece step 18's report deferred here (design-pass
// question 5): a stash held client-side after every mutation, so daemon death or app relaunch
// restores the user's dirty text (H6's intended consumer).
//
// Synchronous by design: every wire op is a blocking µs-scale round-trip (step-18 row D), so
// methods run inline on the caller — the main thread in the app, the test thread in tests.
// Pushes arrive on the connection's callback queue; `apply` marshals the resulting state
// mutations (main-queue dispatch in the app, inline in tests).

import Foundation
import Observation
import SyncWireKit

public enum ConnectionState: Equatable {
    case idle
    case connected
    case disconnected
    case failed(String)
}

/// A submit outcome the view renders — returned data, never a shell-side judgement.
public enum SubmitOutcome: Equatable {
    case submitted(version: UInt64)
    case refusedValidation
    case refusedConflicted(fields: [String])
    case refusedOrphaned
}

@Observable
public final class SyncViewModel {
    public private(set) var connectionState: ConnectionState = .idle
    public private(set) var canonical: CanonicalW?
    public private(set) var draft: DraftW?
    public private(set) var lastSubmit: SubmitOutcome?
    /// The survival blob, refreshed after every mutation (the "continuous stash" idiom — its
    /// very necessity is design-pass evidence, see the report).
    public private(set) var stash: StashW?
    public private(set) var restoredFromStash = false

    // Echo rule (ARCHITECTURE §6): the native control owns its text while focused; these
    // buffers are refreshed from core raw only on blur or external change of an unfocused field.
    public var labelBuffer = ""
    public var folderBuffer = ""
    public var intervalBuffer = ""
    public var focusedField: String?

    // U5 instrumentation: wall-clock nanoseconds per keystroke (trySet + snapshot + stash),
    // measured where the user feels it.
    public private(set) var keystrokeNanos: [UInt64] = []

    private var connection: WireConnection?
    private var socketPath: String?
    private let apply: (@escaping () -> Void) -> Void

    /// `apply` marshals push-driven state mutations: the app passes main-queue dispatch; tests
    /// pass `{ $0() }` to run inline on the callback queue.
    public init(apply: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }) {
        self.apply = apply
    }

    // ---------------------------------------------------------------------------------------------
    // Connection lifecycle + the reconnect story (U4)
    // ---------------------------------------------------------------------------------------------

    public func connect(path: String) {
        socketPath = path
        guard let conn = WireConnection.connect(path: path) else {
            connectionState = .failed("cannot connect to \(path)")
            return
        }
        // Under socket activation, connect(2) success is not daemon liveness (the M4c finding):
        // verify the session with a round-trip before believing it.
        guard conn.request(.ping, timeoutSeconds: 5) != nil else {
            conn.close()
            connectionState = .failed("connected but no pong from \(path)")
            return
        }
        wire(conn)
        connectionState = .connected
        refetchCanonical()
        // Coming back with a survival blob: restore, and say so (the view surfaces it).
        if let blob = stash {
            if let resp = conn.request(.restore(stash: blob)), resp.t == "draft_id" {
                restoredFromStash = true
                refetchDraft(id: resp.draft)
                refreshBuffersFromDraft(exceptFocused: false)
            }
        }
    }

    public func disconnect() {
        connection?.close()
        connection = nil
        connectionState = .idle
    }

    private func wire(_ conn: WireConnection) {
        conn.onPush = { [weak self] push in
            guard let self else { return }
            self.apply { self.handle(push: push) }
        }
        conn.onDisconnect = { [weak self] in
            guard let self else { return }
            self.apply {
                // The daemon is gone and so is its store (step-18 A3) — but the stash is ours.
                self.connection = nil
                self.draft = nil
                self.connectionState = .disconnected
            }
        }
        connection = conn
    }

    private func handle(push: PushW) {
        switch push.t {
        case "canonical_changed":
            refetchCanonical()
        case "draft_rebased":
            // Tick-then-fetch: the push is small; the snapshot is authoritative. Unfocused
            // buffers adopt the rebase; a focused buffer is never overwritten (§6).
            refetchDraft(id: draft.map(\.draft))
            refreshBuffersFromDraft(exceptFocused: true)
            refreshStash()
        default:
            break
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Observe (U1)
    // ---------------------------------------------------------------------------------------------

    private func refetchCanonical() {
        guard let resp = connection?.request(.canonicalSnapshot) else { return }
        canonical = resp.canonical
    }

    private func refetchDraft(id: UInt64?) {
        guard let id, let resp = connection?.request(.draftSnapshot(draft: id)) else { return }
        if let snap = resp.snapshot { draft = snap }
    }

    // ---------------------------------------------------------------------------------------------
    // The editor session (U2/U3)
    // ---------------------------------------------------------------------------------------------

    public func openEditor() {
        guard let conn = connection, draft == nil else { return }
        guard let resp = conn.request(.checkout), resp.t == "draft_id" else { return }
        refetchDraft(id: resp.draft)
        refreshBuffersFromDraft(exceptFocused: false)
        refreshStash()
        lastSubmit = nil
        restoredFromStash = false
    }

    /// Per keystroke: buffer already holds the user's text (the binding wrote it); send it to
    /// the core, refetch the judgement, refresh the OTHER buffers only. Never write the focused
    /// buffer from core — that write-back is what moves the cursor (§6).
    public func edit(field: String, text: String) {
        guard let conn = connection, let id = draft?.draft else { return }
        let t0 = DispatchTime.now().uptimeNanoseconds
        _ = conn.request(.trySet(draft: id, field: field, value: .text(text)))
        refetchDraft(id: id)
        keystrokeNanos.append(DispatchTime.now().uptimeNanoseconds - t0)
        refreshBuffersFromDraft(exceptFocused: true)
        refreshStash()
    }

    public func setPaused(_ on: Bool) {
        guard let conn = connection, let id = draft?.draft else { return }
        _ = conn.request(.trySet(draft: id, field: "paused", value: .flag(on)))
        refetchDraft(id: id)
        refreshStash()
    }

    public func blur(field: String) {
        if focusedField == field { focusedField = nil }
        refreshBuffersFromDraft(exceptFocused: true)
    }

    /// The client IS the checker (the capability sits on this side of the wire — step-18 B2's
    /// shape): begin, judge reachability with the client's own filesystem access, complete.
    /// Debouncing is the view's business (*when*, not *what*).
    public func runFolderCheckIfNeeded() {
        guard let conn = connection, let id = draft?.draft, let snap = draft else { return }
        // Only when the report asks for it — the core owns "does this need checking".
        guard snap.report.ruleKeys.contains("folder_check_required") else { return }
        guard let begun = conn.request(.beginCheck(draft: id, check: "folder_reachable")),
            let token = begun.token
        else { return }
        let path = snap.folder.raw?.asText ?? ""
        var isDir: ObjCBool = false
        let ok = FileManager.default.fileExists(atPath: path, isDirectory: &isDir) && isDir.boolValue
        _ = conn.request(.completeCheck(draft: id, check: "folder_reachable", token: token, ok: ok))
        refetchDraft(id: id)
    }

    public func resolve(field: String, keepMine: Bool) {
        guard let conn = connection, let id = draft?.draft else { return }
        _ = conn.request(.resolve(draft: id, field: field, keepMine: keepMine))
        refetchDraft(id: id)
        refreshBuffersFromDraft(exceptFocused: true)
        refreshStash()
    }

    public func submit() {
        guard let conn = connection, let id = draft?.draft else { return }
        guard let resp = conn.request(.submit(draft: id)) else { return }
        switch resp.t {
        case "submitted":
            lastSubmit = .submitted(version: resp.version ?? 0)
            // The draft is consumed; the canonical pane updates via the push, but fetch now so
            // the UI is honest even before the tick lands.
            draft = nil
            stash = nil
            refetchCanonical()
        case "submit_refused":
            switch resp.refusal?.kind {
            case "conflicted":
                lastSubmit = .refusedConflicted(fields: resp.refusal?.fields ?? [])
            case "orphaned":
                lastSubmit = .refusedOrphaned
            default:
                lastSubmit = .refusedValidation
            }
            // The refusal report is already in the draft's own report — refetch renders it.
            refetchDraft(id: id)
        default:
            break
        }
    }

    public func closeEditor() {
        guard let conn = connection, let id = draft?.draft else { return }
        _ = conn.request(.close(draft: id))
        draft = nil
        stash = nil
        lastSubmit = nil
    }

    // ---------------------------------------------------------------------------------------------
    // The session-less command (G5's app-side twin)
    // ---------------------------------------------------------------------------------------------

    public func togglePaused() {
        guard let conn = connection else { return }
        _ = conn.request(.togglePaused)
        refetchCanonical()
    }

    // ---------------------------------------------------------------------------------------------
    // Buffers + stash
    // ---------------------------------------------------------------------------------------------

    private func refreshBuffersFromDraft(exceptFocused: Bool) {
        guard let d = draft else { return }
        func refresh(_ name: String, _ f: FieldW, _ buffer: inout String) {
            if exceptFocused && focusedField == name { return }
            buffer = f.raw?.asText ?? ""
        }
        refresh("label", d.label, &labelBuffer)
        refresh("folder", d.folder, &folderBuffer)
        refresh("interval", d.interval, &intervalBuffer)
    }

    private func refreshStash() {
        guard let conn = connection, let id = draft?.draft else { return }
        if let resp = conn.request(.stash(draft: id)), let blob = resp.stash {
            stash = blob
        }
    }

    // ---------------------------------------------------------------------------------------------
    // U5: the numbers, where the user feels them
    // ---------------------------------------------------------------------------------------------

    public var keystrokeP50P95Micros: (p50: Double, p95: Double)? {
        guard !keystrokeNanos.isEmpty else { return nil }
        let sorted = keystrokeNanos.sorted()
        let p50 = Double(sorted[sorted.count / 2]) / 1000.0
        let p95 = Double(sorted[min(sorted.count * 95 / 100, sorted.count - 1)]) / 1000.0
        return (p50, p95)
    }
}
