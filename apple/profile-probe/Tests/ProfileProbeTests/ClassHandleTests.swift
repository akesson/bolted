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

    /// Deinit-deregistration: ARC releasing the Swift handle must run Rust `Drop`, which prunes the
    /// draft from the store registry — so `liveDraftCount` falls. If it did not, drafts would leak
    /// and `apply_canonical` would rebase zombies forever (direct evidence for the §9 `close()` Q).
    func testDeinitDeregistration() {
        let store = ProfileStoreFfi()
        XCTAssertEqual(store.liveDraftCount(), 0)
        do {
            let draft = store.checkout()
            XCTAssertEqual(store.liveDraftCount(), 1)
            _ = draft.id()
        } // draft leaves scope → ARC deinit → boltffi release → Rust Drop → deregister
        XCTAssertEqual(store.liveDraftCount(), 0)
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
