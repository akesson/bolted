// The Swift probe driver (step 18). Modes:
//
//   sync-probe connect <socket>            row C — attempt + report verbatim (exit 0 reached,
//                                          exit 2 connect refused, printing the errno)
//   sync-probe cycle <socket>              M3 — one full draft cycle through Codable (B1's script)
//   sync-probe listen-toggle <socket> <s>  row C3 — tick-then-fetch: wait for a canonical tick
//                                          (driven by the RUST client), fetch, verify
//   sync-probe latency <socket> <iters>    row D from Swift (D1 ping, D2 try_set, D3 snapshot,
//                                          D4 keystroke pair), p50/p95 in µs

import Foundation

func fail(_ msg: String) -> Never {
    print("FAIL: \(msg)")
    exit(1)
}

func pass(_ msg: String) {
    print("ok: \(msg)")
}

func usage() -> Never {
    print("usage: sync-probe <connect|cycle|listen-toggle|latency> <socket> [arg]")
    exit(64)
}

let args = CommandLine.arguments
guard args.count >= 3 else { usage() }
let mode = args[1]
let socketPath = args[2]

func mustConnect() -> LineClient {
    switch LineClient.connect(path: socketPath) {
    case .connected(let c): return c
    case .failed(let e, let m): fail("connect \(socketPath): errno=\(e) (\(m))")
    }
}

switch mode {
case "connect":
    // Row C: the outcome IS the datum, both ways. Exit 0 = reached; exit 2 = refused (errno
    // printed verbatim); exit 1 = reached but the protocol failed.
    switch LineClient.connect(path: socketPath) {
    case .failed(let e, let m):
        print("connect-refused errno=\(e) (\(m))")
        exit(2)
    case .connected(let c):
        guard let resp = c.request(.ping), resp.t == "pong" else {
            fail("connected but ping got no pong")
        }
        print("connect-ok pong")
        exit(0)
    }

case "cycle":
    let c = mustConnect()
    guard let pong = c.request(.ping), pong.t == "pong" else { fail("ping") }
    pass("ping")

    guard let co = c.request(.checkout), let draft = co.draft else { fail("checkout") }
    pass("checkout draft=\(draft)")

    // A rejected input is a keyed error with structured params, intact through Codable.
    let long = String(repeating: "x", count: 31)
    guard let bad = c.request(.trySet(draft: draft, field: "label", value: .text(long))),
        let err = bad.error, err.key == "too_long",
        err.params.contains(Param(name: "max", value: "30")),
        err.params.contains(Param(name: "actual", value: "31"))
    else { fail("too_long params through Codable") }
    pass("tier-1 refusal with structured params")

    guard let ok1 = c.request(.trySet(draft: draft, field: "label", value: .text("Swift-Probe"))),
        ok1.error == nil
    else { fail("set label") }
    guard
        let ok2 = c.request(
            .trySet(draft: draft, field: "folder", value: .text("/Volumes/NAS/Swift"))),
        ok2.error == nil
    else { fail("set folder") }
    guard let ok3 = c.request(.trySet(draft: draft, field: "interval", value: .text("5"))),
        ok3.error == nil
    else { fail("set interval") }

    // The tier-2 rule crosses as keyed data.
    guard let v1 = c.request(.validate(draft: draft)), let r1 = v1.report,
        r1.ruleKeys.contains("network_interval_too_fast")
    else { fail("tier-2 rule over the wire") }
    pass("tier-2 rule fired (network_interval_too_fast)")

    guard let ok4 = c.request(.trySet(draft: draft, field: "interval", value: .text("30"))),
        ok4.error == nil
    else { fail("fix interval") }

    // C16 as data, then the single-flight pair with a stale token discarded.
    guard let v2 = c.request(.validate(draft: draft)), let r2 = v2.report,
        r2.ruleKeys.contains("folder_check_required")
    else { fail("folder_check_required expected") }
    guard let b1 = c.request(.beginCheck(draft: draft, check: "folder_reachable")),
        let stale = b1.token
    else { fail("begin 1") }
    guard let b2 = c.request(.beginCheck(draft: draft, check: "folder_reachable")),
        let fresh = b2.token
    else { fail("begin 2") }
    guard
        let s1 = c.request(
            .completeCheck(draft: draft, check: "folder_reachable", token: stale, ok: true)),
        s1.accepted == false
    else { fail("stale token must settle nothing") }
    guard
        let s2 = c.request(
            .completeCheck(draft: draft, check: "folder_reachable", token: fresh, ok: true)),
        s2.accepted == true
    else { fail("fresh token settles") }
    pass("single-flight across the wire (stale discarded)")

    guard let v3 = c.request(.validate(draft: draft)), let r3 = v3.report, r3.isOk else {
        fail("draft should be green")
    }
    guard let sub = c.request(.submit(draft: draft)), let newVersion = sub.version else {
        fail("submit")
    }
    pass("submitted version=\(newVersion)")

    guard let can = c.request(.canonicalSnapshot), let canon = can.canonical,
        canon.label == "Swift-Probe", canon.folder == "/Volumes/NAS/Swift"
    else { fail("canonical shows the submit") }
    pass("canonical fetched (label=\(canon.label))")
    print("CYCLE-OK")

case "listen-toggle":
    let timeout = args.count > 3 ? Int(args[3]) ?? 30 : 30
    switch LineClient.connect(path: socketPath, timeoutSeconds: timeout) {
    case .failed(let e, let m):
        fail("connect: errno=\(e) (\(m))")
    case .connected(let c):
        guard let before = c.request(.canonicalSnapshot), let b = before.canonical else {
            fail("initial canonical fetch")
        }
        print("waiting version=\(b.version) paused=\(b.paused)")
        guard let push = c.waitPush(), push.t == "canonical_changed", let v = push.version else {
            fail("no canonical tick arrived within \(timeout)s")
        }
        guard let after = c.request(.canonicalSnapshot), let a = after.canonical,
            a.version == v, a.version > b.version, a.paused != b.paused
        else { fail("tick-then-fetch mismatch") }
        print("C3-OK tick v=\(v) paused \(b.paused)->\(a.paused)")
    }

case "latency":
    let iters = args.count > 3 ? Int(args[3]) ?? 1000 : 1000
    let c = mustConnect()
    guard let co = c.request(.checkout), let draft = co.draft else { fail("checkout") }

    func percentiles(_ label: String, _ samples: [UInt64]) {
        let sorted = samples.sorted()
        let p50 = sorted[sorted.count / 2]
        let p95 = sorted[min(sorted.count - 1, (sorted.count * 95) / 100)]
        print(
            "\(label) n=\(sorted.count) p50_us=\(Double(p50) / 1000.0) p95_us=\(Double(p95) / 1000.0)"
        )
    }

    func measure(_ n: Int, _ op: (Int) -> Bool) -> [UInt64] {
        var out: [UInt64] = []
        out.reserveCapacity(n)
        for i in 0..<n {
            let t0 = DispatchTime.now().uptimeNanoseconds
            guard op(i) else { fail("latency op failed at \(i)") }
            out.append(DispatchTime.now().uptimeNanoseconds - t0)
        }
        return out
    }

    // Warm-up, then the four D rows.
    _ = measure(100) { _ in c.request(.ping)?.t == "pong" }
    percentiles("D1_ping", measure(iters) { _ in c.request(.ping)?.t == "pong" })
    percentiles(
        "D2_try_set",
        measure(iters) { i in
            c.request(.trySet(draft: draft, field: "label", value: .text("Swift-\(i)")))?.error
                == nil
        })
    percentiles(
        "D3_snapshot", measure(iters) { _ in c.request(.draftSnapshot(draft: draft)) != nil })
    percentiles(
        "D4_keystroke_pair",
        measure(iters) { i in
            guard
                let set = c.request(
                    .trySet(draft: draft, field: "label", value: .text("Swift-\(i)"))),
                set.error == nil
            else { return false }
            return c.request(.draftSnapshot(draft: draft)) != nil
        })
    print("LATENCY-OK")

default:
    usage()
}
