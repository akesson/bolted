import XCTest

/// End-to-end UI tests for the step-03 manual protocol, driving the real AppKit event pipeline
/// through the accessibility identifiers in ProfileForm.swift. Each test launches the app fresh
/// (seed: alice / Alice Smith / alice@example.com). Requires the host app to hold Accessibility
/// permission (see the step-03 report).
///
/// Coverage split — what XCUITest can drive deterministically vs. what it can't:
///   • item 1 (cursor survival while typing fast): manual only — XCUITest can't read the caret.
///   • item 2 UNFOCUSED clean adopt: here (test2). The FOCUSED §9 case (stale-until-blur) can't be
///     driven through real clicks (focus/blur can't be ordered against the async rebase), so it is
///     verified deterministically in ProfileViewModelTests.testLiveRebaseFocusedCleanFieldStaleUntilBlur.
///   • items 3–6: here.
///
/// Note: SwiftUI Text/TextField expose their content via `.value` (not `.label`) to XCUITest.
final class ProfileUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.staticTexts["Edit profile"].waitForExistence(timeout: 15))
    }

    override func tearDownWithError() throws {
        app.terminate()
        app = nil
    }

    // MARK: item 2 — live rebase adopts a clean field (unfocused)

    func test2_liveRebase_cleanFieldAdoptsSilently() {
        let name = app.textFields["field-name"]
        XCTAssertEqual(value(name), "Alice Smith")

        app.buttons["sim-name"].click() // rebase while name is NOT focused
        expectValue(name, equals: "Server Name", timeout: 3,
                    "a clean, unfocused field silently adopts the server value on rebase")
        XCTAssertFalse(app.staticTexts["conflict-theirs-name"].exists,
                       "a clean adopt must NOT raise a conflict")
    }

    // MARK: item 3 — dirty conflict banner + keep-mine / take-theirs

    func test3a_dirtyConflict_keepMine() {
        let name = app.textFields["field-name"]
        edit(name, to: "My Name")
        app.textFields["field-username"].click() // blur → the edit is dirty against base

        app.buttons["sim-name"].click()
        XCTAssertTrue(app.staticTexts["conflict-theirs-name"].waitForExistence(timeout: 3),
                      "a dirty field that the server changes must conflict")
        XCTAssertEqual(value(app.staticTexts["conflict-theirs-name"]), "Server Name")
        XCTAssertEqual(value(name), "My Name", "mine is preserved under conflict")

        app.buttons["keepmine-name"].click()
        XCTAssertTrue(waitGone(app.staticTexts["conflict-theirs-name"], 3),
                      "keep-mine clears the conflict")
        XCTAssertEqual(value(name), "My Name")
    }

    func test3b_dirtyConflict_takeTheirs() {
        let name = app.textFields["field-name"]
        edit(name, to: "My Name")
        app.textFields["field-username"].click()

        app.buttons["sim-name"].click()
        XCTAssertTrue(app.staticTexts["conflict-theirs-name"].waitForExistence(timeout: 3))

        app.buttons["taketheirs-name"].click()
        XCTAssertTrue(waitGone(app.staticTexts["conflict-theirs-name"], 3),
                      "take-theirs clears the conflict")
        expectValue(name, equals: "Server Name", timeout: 3, "take-theirs adopts their value")
    }

    // MARK: item 4 — C14: a conflicted field edited to EQUAL theirs auto-converges

    func test4_C14_editToEqualTheirs_autoConverges() {
        let user = app.textFields["field-username"]
        edit(user, to: "xyz")
        app.textFields["field-name"].click() // blur → dirty

        app.buttons["sim-username"].click()
        XCTAssertTrue(app.staticTexts["conflict-theirs-username"].waitForExistence(timeout: 3))
        XCTAssertEqual(value(app.staticTexts["conflict-theirs-username"]), "server_user")

        // Type our value until it EQUALS theirs ("server_user").
        edit(user, to: "server_user")
        app.textFields["field-name"].click() // blur

        // C14 (was F6): two edits that agree are not a conflict, whichever arrived first. Before the
        // freeze the banner stayed, with "Keep mine" and "Take theirs" doing visibly the same thing.
        XCTAssertFalse(app.staticTexts["conflict-theirs-username"].exists,
                       "C14: editing to theirs must clear the conflict banner")
        XCTAssertFalse(app.staticTexts["dirty-username"].exists,
                       "C14: and the field lands clean")
    }

    // MARK: item 5 — async uniqueness check: taken → error, valid → clean; spinner best-effort

    func test5_asyncCheck_takenSurfacesError_validDoesNot() {
        let user = app.textFields["field-username"]

        // A valid, unique username: no taken-error, and any spinner clears.
        edit(user, to: "freshname")
        let spinner = app.progressIndicators["spinner-username"]
        if spinner.waitForExistence(timeout: 3) {
            XCTAssertTrue(waitGone(spinner, 5), "spinner must clear once the check completes")
        } // an indeterminate spinner's element exposure varies — appearance is best-effort
        XCTAssertFalse(app.staticTexts["error-username"].waitForExistence(timeout: 3),
                       "a unique username surfaces no taken-error")

        // A taken username ("admin") must surface the inline error after the async verdict.
        edit(user, to: "admin")
        XCTAssertTrue(app.staticTexts["error-username"].waitForExistence(timeout: 5),
                      "a taken username surfaces an inline error after the check")
    }

    // MARK: item 6 — submit: invalid / conflicted / clean, and a failed submit keeps the draft alive

    func test6a_submitInvalid_reportsValidation() {
        let name = app.textFields["field-name"]
        clear(name) // required field now empty
        app.textFields["field-username"].click() // blur

        app.buttons["submit"].click()
        XCTAssertTrue(app.staticTexts["submit-validation"].waitForExistence(timeout: 3),
                      "submitting with an invalid required field is refused with a validation report")
    }

    func test6b_submitConflicted_thenResolve_thenSucceeds() {
        // A conflicted submit is refused AND leaves the draft alive (F3): resolving then resubmitting
        // must succeed on the SAME draft.
        let name = app.textFields["field-name"]
        edit(name, to: "My Name")
        app.textFields["field-username"].click()
        app.buttons["sim-name"].click()
        XCTAssertTrue(app.staticTexts["conflict-theirs-name"].waitForExistence(timeout: 3))

        app.buttons["submit"].click()
        XCTAssertTrue(app.staticTexts["submit-conflicted"].waitForExistence(timeout: 3),
                      "an unresolved conflict refuses submit")

        app.buttons["keepmine-name"].click() // draft still alive → resolve
        app.buttons["submit"].click()
        XCTAssertTrue(app.staticTexts["submit-success"].waitForExistence(timeout: 3),
                      "F3: the draft survived the refusal, so resolve + resubmit succeeds")
    }

    func test6c_submitClean_succeedsAndUpdatesCanonical() {
        let email = app.textFields["field-email"]
        edit(email, to: "new@corp.example")
        app.textFields["field-username"].click() // blur

        app.buttons["submit"].click()
        XCTAssertTrue(app.staticTexts["submit-success"].waitForExistence(timeout: 3),
                      "a clean edit submits successfully")
        // Success propagates to canonical via the store stream, and the editor re-checks-out.
        expectValueContains(app.staticTexts["canonical-email"], "new@corp.example", timeout: 3,
                            "a successful submit updates the canonical (server) snapshot")
    }

    // MARK: helpers

    private func value(_ el: XCUIElement) -> String { (el.value as? String) ?? "" }

    /// Focus, select-all, delete, then type — drives the real per-keystroke binding path.
    private func edit(_ el: XCUIElement, to text: String) {
        clear(el)
        el.typeText(text)
    }

    private func clear(_ el: XCUIElement) {
        el.click()
        el.typeKey("a", modifierFlags: .command)
        el.typeKey(.delete, modifierFlags: [])
    }

    private func waitGone(_ el: XCUIElement, _ timeout: TimeInterval) -> Bool {
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
                                             object: el)
        return XCTWaiter().wait(for: [gone], timeout: timeout) == .completed
    }

    private func expectValue(_ el: XCUIElement, equals expected: String,
                             timeout: TimeInterval, _ message: String) {
        let pred = NSPredicate(format: "value == %@ OR label == %@", expected, expected)
        let exp = XCTNSPredicateExpectation(predicate: pred, object: el)
        XCTAssertEqual(XCTWaiter().wait(for: [exp], timeout: timeout), .completed, message)
    }

    private func expectValueContains(_ el: XCUIElement, _ substring: String,
                                     timeout: TimeInterval, _ message: String) {
        let pred = NSPredicate(format: "value CONTAINS %@ OR label CONTAINS %@", substring, substring)
        let exp = XCTNSPredicateExpectation(predicate: pred, object: el)
        XCTAssertEqual(XCTWaiter().wait(for: [exp], timeout: timeout), .completed, message)
    }
}
