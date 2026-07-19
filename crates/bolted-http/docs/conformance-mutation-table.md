# bolted-http conformance — the mutation pass (step 24 M4)

The suite bites or it doesn't. This table is the evidence that it does: for **each implementor**
(the socket mock, and the reqwest reference adapter `bolted-http-linux`) a set of behaviour
mutations that a correct suite must catch, the row(s) that caught each, and the exact typed
`FailureReason` observed. Two mutations survived — each is discharged on the *first* hypothesis (the
mutant was semantically identical) per [[a-surviving-mutation-is-two-hypotheses]], not left as a
false "blind spot". One genuine blind spot was found and fixed (a new committed row); one
positive-control gap was filled.

**The mutations themselves are not committed** — they were applied one at a time, run, recorded, and
reverted; the working tree ends clean except the suite-strengthening (`c1.rs`, `mod.rs`, `netmock.rs`)
and this file. Where a mutation is a permanent `MockBehavior` knob, it stays as documented red-twin
test surface (its red-twin test is committed).

## How to read a row

- **Implementor** — `mock` (socket mock, `netmock.rs`) or `reqwest` (`bolted-http-linux`).
- **Vehicle** — an existing `MockBehavior` knob (permanent, with a committed red-twin test), a
  **new** M4 knob, or a **temp src** one-line source edit (applied, run, reverted).
- **Caught by / reason** — the row that went red and the typed `FailureReason` it produced.

## Mock (socket mock) — every §7 rule + the matrix rows

| # | Mutation | Vehicle | Caught by | Reason observed |
|---|----------|---------|-----------|-----------------|
| M1 | Non-deterministic body | knob `deterministic=false` | C1/rule-01 | `NotDeterministic` |
| M2 | Cancel reported as Timeout | knob `classify_cancel=false` | C1/rule-02 | `KeysNotDistinct{Timeout}` |
| M3 | No deadline (stalled body hangs) | knob `arm_deadline=false` | C1/rule-03, C2/key-timeout | `NoCompletion` / row-fail |
| M4 | https→http redirect followed | knob `refuse_insecure_redirect=false` | C1/rule-04, C2/key-insecure-redirect | `ExpectedErrorGotSuccess{InsecureRedirect}` |
| M5 | Cache-served 200 instead of real 304 | knob `send_headers=false` (drops `If-None-Match`) | C1/rule-05 | `UnexpectedStatus{expected:304,got:200}` |
| M6 | Request header silently dropped | knob `send_headers=false` | C1/rule-06 | `MissingHeader{x-trace-id}` |
| M7 | gzip left undecoded | knob `decode_gzip=false` | C1/rule-07 | `WrongBody` |
| M8 | Hidden retry on transport failure | knob `retry_on_transport=true` | C1/rule-08 | `HiddenRetry{connections:2}` |
| M9 | Cancel never completes (silence) | knob `honor_cancel=false` | C1/rule-09, C2/key-cancelled | `NoCompletion` / row-fail |
| M10 | Pin bypass (accept any cert) | knob `check_pins=false` | C1/rule-10, C2/key-pin-mismatch | `ExpectedErrorGotSuccess{PinMismatch}` |
| M11 | Dishonest `content_length` under decoding | knob `honest_content_length=false` | C1/rule-07 | `DishonestContentLength{got:compressed}` |
| M12 | Non-monotone upload progress | knob `honest_upload_progress=false` | C1/rule-11 | `ProgressNotMonotone` |
| M13 | Sink ignored (Memory for File) | knob `honor_file_sink=false` | C1/row-15, C2/key-io | `WrongSink` / row-fail |
| M14 | Untrusted cert trusted | factory `correct(untrusted_spki)` | C2/key-tls | row-fail (success where `Tls` required) |
| M15 | No redirect ceiling (chases loop) | knob `redirect_limit=u32::MAX` | C2/key-too-many-redirects | `NoCompletion` |
| **M16** | **Timeout misclassified as Transport** | **temp src**: `map_io_error` `Stop::Deadline ⇒ Transport` | C1/rule-03, C2/key-timeout | `WrongErrorKey{expected:Timeout,got:Transport}` |
| **M17** | **Upload progress stops one chunk short (monotone but never terminal)** | **new knob** `terminal_upload_progress=false` | C1/rule-11 | `ProgressNotTerminal` |
| **M18** | **Redirect followed but final URL + hop trace misreported** | **new knob** `honest_redirect_trace=false` | **C1/row-redirect-trace (NEW)** | `WrongHopTrace{got:0,expected:2}` |

M16 covers the required "wrong error key (timeout reported as Transport)" case that no permanent knob
expressed: it is a one-line temp source edit, reverted. M17/M18 are the two additions below.

## reqwest reference adapter (`bolted-http-linux`) — temp source mutations

Each was a single source edit, the full suite run, the red row + reason recorded, then reverted (the
adapter source has **zero** diff at the end — verified with `git diff`).

| # | Mutation | Source edit | Caught by | Reason observed | Result |
|---|----------|-------------|-----------|-----------------|--------|
| A1 | Comment out the pin AND (accept any pin) | `tls.rs`: `if false && !pins.contains(&spki)` | C1/rule-10, C2/key-pin-mismatch, L2 | `ExpectedErrorGotSuccess{PinMismatch,200}` | **caught** |
| A2 | reqwest default redirect policy (follows https→http) | `lib.rs`: `Policy::none() → Policy::default()` | C1/rule-04, C2/key-insecure-redirect | `ExpectedErrorGotSuccess{InsecureRedirect,200}` | **caught** |
| A3 | Report cancel as Timeout | `lib.rs`: cancel arm `Err(Cancelled) → Err(Timeout)` | C1/rule-02, C2/key-cancelled (+rule-09) | `KeysNotDistinct{Timeout}` / `WrongErrorKey{Cancelled←Timeout}` | **caught** |
| A4a | Skip the atomic rename (leave body at temp path) | `lib.rs`: drop `fs::rename`, return `Ok(())` | C1/row-15 | `WrongSink` | **caught** |
| A4b | Skip the `fsync` only (keep rename) | `lib.rs`: drop `sync_all()` | — | — | **survived → hyp. 2** |
| A5 | Drop all request headers | `lib.rs`: skip the header-append loop | C1/rule-05 (rule-06 also guards) | `UnexpectedStatus{304,200}` | **caught** |
| A6 | Re-enable pooled retry | `lib.rs`: drop `retry(never())` + `pool_max_idle_per_host(0)` | — | — | **survived → hyp. 2** |
| A7 | Drop the redirect hop from the trace | `lib.rs`: drop `hops.push(...)` | **C1/row-redirect-trace (NEW)** | `WrongHopTrace{got:0,expected:2}` | **caught** |

## Survivors — the two-hypotheses discharge

Per [[a-surviving-mutation-is-two-hypotheses]], a survivor is either (1) a suite blind spot or (2) a
mutant that changed nothing observable. Both survivors here are (2), proven — not reported as holes.

### A4b — skip the `fsync`, keep the rename (survived)

The bytes at `target` are correct the instant `rename` completes; `sync_all` only changes whether
they persist across a **crash / power loss**. A single-process conformance suite reads the file back
in the same process, so the mutant is **behaviourally identical** to the original for anything the
suite can observe. Discharged as hypothesis 2. Verifying fsync would need a crash/kill harness —
out of scope, and not a suite blind spot. **No test added** (adding one would assert the mutant's own
behaviour — exactly the trap the lesson warns against). The `sync_all` line stays as recorded
crash-durability discipline that the in-process suite cannot, and should not pretend to, verify.

### A6 — re-enable pooled retry (survived)

`server.hits("/flaky") == 1` after the mutation: the server saw **exactly one** connection, so no
retry occurred. Two reasons, both making the mutation vacuous *by construction*:

1. `/flaky` truncates **mid-body** after a `200` header. reqwest's auto-retry only re-sends requests
   that failed **before** a response was received; a truncated-after-200 body is never retried.
2. The adapter builds a **fresh reqwest client per request** (pin data is per-request), so no idle
   pooled connection ever exists — the only condition reqwest's pooled-connection retry targets.

So removing `retry(never())` + `pool_max_idle_per_host(0)` changes nothing the suite (or this
adapter's own architecture) can exercise: they are defense-in-depth against a condition the
per-request-client design already precludes. Discharged as hypothesis 2. The suite is **not** blind
to a retry that *does* happen — the mock's `retry_on_transport` knob (M8) drives a real
transport-level retry and `rule_08` catches it (`HiddenRetry`). **No test added**: manufacturing
reqwest's pooled-retry condition would require defeating the adapter's own client-per-request design,
and the suite correctly reflects that the design makes hidden request-level retry unreachable.

## The blind spot found and fixed

### The redirect trace observables (`final_url` + `hops`) had no positive control

**What survived (before M4):** a mutation that follows a redirect chain but reports the **original
request URL** as `final_url` and **drops the hop trace** passed the *entire* suite. `HttpResponse`
carries `final_url()` and `hops()` as contract observables (M0/M1), and the test server hosts
`/redirect-chain?n=N`, but **no row referenced either accessor** (confirmed by grep across the
conformance module). Redirects were only ever tested for their two *refusal* cases — https→http
(rule 4) and the loop ceiling (C2 too-many-redirects) — never for a *successful* chain's reported
endpoint.

**Two-hypotheses check:** the mutant is observably different from the correct adapter — the correct
socket mock and reqwest adapter both report `final_url` ending in `n=0` with `hops.len() == 2`, while
the mutant reports `final_url` ending in `n=2` with `hops.len() == 0`. So this is hypothesis 1 (a
real behaviour the suite was blind to), not a vacuous mutant.

**The fix (committed):** a new C1-adjacent row `C1/row-redirect-trace-final-url-and-hops` in
`extra_rows()` drives `/redirect-chain?n=2` and asserts status `200`, `hops().len() == 2`, and
`final_url()` is the chain's tail (`n=0`). It runs against **both** implementors (the mock suite and
the reqwest suite chain `extra_rows()`). New typed `FailureReason`s `WrongFinalUrl` / `WrongHopTrace`
carry the data. Watched **red** two ways:

- mock: new knob `honest_redirect_trace=false` → `WrongHopTrace{got:0,expected:2}`
  (`redirect_trace_red_when_trace_dropped`).
- reqwest: temp mutation A7 (drop `hops.push`) → same reason on the real adapter.

Both correct implementors pass the new row green.

## The positive-control gap filled

`judge_progress` (rule 11) has two failure branches — `ProgressNotMonotone` and
`ProgressNotTerminal` — but only the first was ever watched red (the `honest_upload_progress=false`
twin jumps to 100% then drops, so its *final* sample equals the body length and the terminal branch
never fires). A **monotone-but-short** sequence — the common "forgot to report the last chunk" bug —
was never exercised. This is a positive-control gap (the branch is correct, but unproven), not a
survivor: the new knob `terminal_upload_progress=false` reports monotone progress that stops one
chunk short, and `rule_11` duly goes red with `ProgressNotTerminal`
(`rule_11_red_when_progress_stops_short`, committed).

## Summary

| Implementor | Mutations | Caught | Survived (hyp. 2) | Blind spot fixed | Positive-control gap filled |
|-------------|-----------|--------|-------------------|------------------|------------------------------|
| mock | 18 | 18 | 0 | 1 (redirect trace) | 1 (progress terminal) |
| reqwest | 8 | 6 | 2 (A4b, A6) | (shares the redirect-trace row) | — |

No surviving mutation is left unexplained; both survivors are proven hypothesis 2 (semantically
identical to the suite), not blind spots. The suite strengthening — the redirect-trace row + its two
`FailureReason`s, and the two new mock knobs with their red-twin tests — is committed; the mutations
are not.
