import XCTest
import GenProfileFfi

/// Feature 1 — classes / handles. Records observed behaviour, not just success.
final class ClassHandleTests: XCTestCase {
    /// Methods on a returned draft object work, and mutate observable state.
    func testMethodsOnReturnedDraftWork() throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        try draft.trySetUsername(raw: "  alice  ") // value type trims
        let snap = draft.snapshot()
        XCTAssertEqual(snap.username.validity, .valid(value: "alice"))
        XCTAssertTrue(snap.username.dirty) // create-flow: set field is dirty (no base)
    }

    /// Handle round-trip identity: passing the draft back as a parameter reaches the SAME Rust
    /// object (BoltFFI forwards the handle), so ids-not-instances is NOT forced onto the contract.
    func testHandleRoundTripIdentity() {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        XCTAssertEqual(store.sameDraft(other: draft), draft.id())
    }

    /// Deinit-deregistration — **D26 leak-freedom, the Apple half.** ARC releasing the Swift handle
    /// must run Rust `Drop`, which prunes the draft from the store registry, so tearing down whatever
    /// owns a draft returns `liveDraftCount` (C22) to its baseline. If it did not, drafts would leak
    /// and `apply_canonical` would rebase zombies forever.
    ///
    /// The step-12 design pass (D26) declined a `Cleaner`-style backstop; this test **is** the
    /// backstop it chose instead — a forgotten handle here would leak, and the count would not return
    /// to baseline. Baseline is captured rather than assumed 0, so the contract reads as
    /// "teardown → baseline", not "teardown → empty".
    func testDeinitDeregistration() {
        let store = ProfileStoreFfi()
        let baseline = store.liveDraftCount()
        do {
            let draft = store.checkout()
            XCTAssertEqual(store.liveDraftCount(), baseline + 1)
            _ = draft.id()
        } // draft leaves scope → ARC deinit → boltffi release → Rust Drop → deregister
        XCTAssertEqual(store.liveDraftCount(), baseline, "teardown must return to baseline (D26)")
    }

    /// Two live drafts each register; dropping one leaves the other.
    func testMultipleDraftsCountIndependently() {
        let store = ProfileStoreFfi()
        let keep = store.checkout()
        do {
            let temp = store.checkout()
            XCTAssertEqual(store.liveDraftCount(), 2)
            _ = temp.id()
        }
        XCTAssertEqual(store.liveDraftCount(), 1)
        _ = keep.id()
    }

    /// A submitted draft becomes an inert tombstone: `isLive()` is false, a mutating call refuses
    /// with the typed `draftClosed` (D23 — before step 10 this was a silent no-op), a second submit
    /// reports `AlreadySubmitted`, and it is not "live". This is why `submit` never needs to
    /// consume the foreign handle (partly dissolving F3/Q6 at the FFI layer) — recorded, not fixed
    /// here.
    func testPostSubmitTombstone() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertTrue(draft.isLive())

        try draft.submit()

        XCTAssertFalse(draft.isLive())
        XCTAssertThrowsError(try draft.trySetUsername(raw: "ignored")) { error in
            XCTAssertEqual(error as? UsernameErrorFfi, .draftClosed) // D23: typed, not silent
        }
        XCTAssertEqual(store.liveDraftCount(), 0)
        XCTAssertThrowsError(try draft.submit()) { error in
            XCTAssertEqual(error as? SubmitErrorFfi, .alreadySubmitted)
        }
    }
}
