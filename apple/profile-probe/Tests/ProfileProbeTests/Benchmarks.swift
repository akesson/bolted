import XCTest
import SpikeProfileFfi

/// Measurements — recorded, NOT gated. Apple overhead is not evidence for the JNI per-keystroke
/// bet (step 05 re-measures on Android, VISION's worst case); this is only a baseline. Each
/// `measure {}` runs 1000 calls; divide the reported time by 1000 for the per-call cost.
final class Benchmarks: XCTestCase {
    /// The per-keystroke `try_set` round-trip (encode raw → cross → validate → emit snapshot).
    func testTrySetUsernameThroughput() {
        let draft = ProfileStoreFfi().checkout()
        measure {
            for i in 0..<1000 {
                try? draft.trySetUsername(raw: "user\(i % 900 + 100)")
            }
        }
    }

    /// The snapshot read-back (`snapshot()`) round-trip — marshaling the whole ProfileSnapshot DTO.
    func testSnapshotReadbackThroughput() throws {
        let draft = ProfileStoreFfi().checkout()
        try draft.trySetUsername(raw: "alice")
        measure {
            for _ in 0..<1000 {
                _ = draft.snapshot()
            }
        }
    }
}
