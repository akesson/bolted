import XCTest
import SpikeProfileFfi

/// Feature 3 — `Result` methods with typed error enums. The point is that STRUCTURED payloads
/// (associated values, and the nested validation report) survive to Swift, not flattened strings.
final class TypedErrorTests: XCTestCase {
    /// A tier-1 setter throws a typed enum with associated values Swift can read.
    func testSetterThrowsTypedTooShort() {
        let draft = ProfileStoreFfi().checkout()
        XCTAssertThrowsError(try draft.trySetUsername(raw: "ab")) { error in
            XCTAssertEqual(error as? UsernameErrorFfi, .tooShort(min: 3, actual: 2))
        }
    }

    func testSetterThrowsInvalidChars() {
        let draft = ProfileStoreFfi().checkout()
        XCTAssertThrowsError(try draft.trySetUsername(raw: "has space")) { error in
            XCTAssertEqual(error as? UsernameErrorFfi, .invalidChars)
        }
    }

    /// The composite value object's error carries its two dates as associated values, and the
    /// setter takes two arguments — never a tuple.
    func testDateRangeSetterThrowsTyped() {
        let draft = ProfileStoreFfi().checkout()
        let start = PlainDate(year: 2026, month: 12, day: 31)
        let end = PlainDate(year: 2026, month: 1, day: 1)
        XCTAssertThrowsError(try draft.trySetAvailability(start: start, end: end)) { error in
            XCTAssertEqual(error as? DateRangeErrorFfi, .startAfterEnd(start: start, end: end))
        }
    }

    /// A unit-only `#[error]` enum crosses as a C-style (raw-value) Swift enum, still `Error`.
    func testEmailErrorIsTyped() {
        let draft = ProfileStoreFfi().checkout()
        XCTAssertThrowsError(try draft.trySetEmail(raw: "no-at-sign")) { error in
            XCTAssertEqual(error as? EmailErrorFfi, .invalid)
        }
    }

    /// Submit throws `SubmitErrorFfi.validation` carrying the full structured report — field ids +
    /// keyed `ErrorData` — readable in Swift, not a message string. A create-flow draft is all
    /// `Unset`, so every field is `required`.
    func testSubmitThrowsValidationWithReportPayload() {
        let draft = ProfileStoreFfi().checkout()
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .validation(let report)? = error as? SubmitErrorFfi else {
                return XCTFail("expected .validation, got \(error)")
            }
            XCTAssertEqual(report.fieldErrors.count, 4)
            XCTAssertEqual(report.fieldError(.username)?.key, "required")
            XCTAssertEqual(report.fieldError(.email)?.key, "required")
        }
    }

    /// A field error with PARAMS survives inside the report (the `too_short` {min, actual} data).
    func testReportCarriesFieldErrorParams() {
        let draft = ProfileStoreFfi().checkout()
        XCTAssertThrowsError(try draft.trySetUsername(raw: "ab")) // records Invalid, also throws
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .validation(let report)? = error as? SubmitErrorFfi else {
                return XCTFail("expected .validation, got \(error)")
            }
            let usernameError = report.fieldError(.username)
            XCTAssertEqual(usernameError?.key, "too_short")
            XCTAssertEqual(usernameError?.param("min"), "3")
            XCTAssertEqual(usernameError?.param("actual"), "2")
        }
    }

    /// The tier-2 relational rule (`corporate_email`) reaches the report with its params.
    func testTier2RuleViolationInReport() throws {
        let draft = ProfileStoreFfi().checkout()
        try draft.trySetUsername(raw: "corp_bob")
        try draft.trySetName(raw: "Bob")
        try draft.trySetEmail(raw: "bob@gmail.com") // not corp.example → rule fires
        try draft.trySetAvailability(
            start: PlainDate(year: 2026, month: 1, day: 1),
            end: PlainDate(year: 2026, month: 2, day: 1)
        )
        let report = draft.validate()
        XCTAssertTrue(report.ruleNames.contains("corporate_email"))
        let violation = report.ruleErrors.first { $0.rule == "corporate_email" }
        XCTAssertEqual(violation?.error.param("expected"), "corp.example")
        XCTAssertEqual(violation?.pins, [.email])
    }
}
