import Foundation
import GenProfileFfi

// Shared fixtures + tiny helpers for the probe tests.

/// A valid canonical seed (no `corp_` prefix, so the tier-2 `corporate_email` rule stays quiet).
func validValues() -> ProfileValues {
    ProfileValues(
        username: "alice",
        name: "Alice Smith",
        email: "alice@example.com",
        availability: AvailabilityRaw(
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

// --- Username-checker capability stubs (implemented on the Swift side) ---

/// Returns a fixed verdict.
final class StubChecker: UsernameChecker {
    let verdict: CheckVerdictFfi
    init(_ verdict: CheckVerdictFfi) { self.verdict = verdict }
    func check(value: String) -> CheckVerdictFfi { verdict }
}

/// Returns scripted verdicts in order (the D34 shape of "swap the checker": one checker whose
/// behaviour evolves, supplied once at checkout — the draft's capability never changes identity).
final class SequencedChecker: UsernameChecker {
    private var verdicts: [CheckVerdictFfi]
    init(_ verdicts: [CheckVerdictFfi]) { self.verdicts = verdicts }
    func check(value: String) -> CheckVerdictFfi {
        verdicts.isEmpty ? .pass : verdicts.removeFirst()
    }
}

/// Synchronously re-enters the SAME draft from inside the callback — the reentrancy/deadlock probe.
final class ReentrantChecker: UsernameChecker {
    weak var draft: ProfileDraftFfi?
    var reentered = false
    func check(value: String) -> CheckVerdictFfi {
        if let draft {
            _ = draft.validate() // a reentrant READ (locks the store's Mutex)
            try? draft.trySetName(raw: "Reentrant") // a reentrant MUTATION (locks + emits)
            reentered = true
        }
        return .pass
    }
}
