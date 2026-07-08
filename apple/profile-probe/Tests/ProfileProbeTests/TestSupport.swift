import Foundation
import SpikeProfileFfi

// Shared fixtures + tiny helpers for the probe tests.

/// A valid canonical seed (no `corp_` prefix, so the tier-2 `corporate_email` rule stays quiet).
func validValues() -> ProfileValues {
    ProfileValues(
        username: "alice",
        name: "Alice Smith",
        email: "alice@example.com",
        availability: PlainDateRange(
            start: PlainDate(year: 2026, month: 1, day: 1),
            end: PlainDate(year: 2026, month: 12, day: 31)
        )
    )
}

extension ValidationReportFfi {
    var isOK: Bool { fieldErrors.isEmpty && ruleErrors.isEmpty }
    func fieldError(_ field: ProfileFieldId) -> ErrorData? {
        fieldErrors.first { $0.field == field }?.error
    }
    var ruleNames: [String] { ruleErrors.map(\.rule) }
}

extension ErrorData {
    func param(_ key: String) -> String? {
        params.first { $0.key == key }?.value
    }
}

/// A mutable reference box for capturing a snapshot out of an async consuming `Task`. Reads happen
/// only after the fulfilling `await`, i.e. happens-after the write — hence `@unchecked Sendable`.
final class SnapshotBox: @unchecked Sendable {
    var value: ProfileSnapshot?
}

// --- Uniqueness-checker capability stubs (implemented on the Swift side) ---

/// Returns a fixed verdict.
final class StubChecker: UniquenessChecker {
    let verdict: UniquenessVerdictFfi
    init(_ verdict: UniquenessVerdictFfi) { self.verdict = verdict }
    func checkUnique(username: String) -> UniquenessVerdictFfi { verdict }
}

/// Synchronously re-enters the SAME draft from inside the callback — the reentrancy/deadlock probe.
final class ReentrantChecker: UniquenessChecker {
    weak var draft: ProfileDraftFfi?
    var reentered = false
    func checkUnique(username: String) -> UniquenessVerdictFfi {
        if let draft {
            _ = draft.validate() // a reentrant READ (locks the store's Mutex)
            try? draft.trySetName(raw: "Reentrant") // a reentrant MUTATION (locks + emits)
            reentered = true
        }
        return .unique
    }
}
