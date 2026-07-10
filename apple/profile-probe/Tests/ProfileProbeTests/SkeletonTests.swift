import XCTest
import GenProfileFfi

/// Milestone 1 — the walking skeleton. Proves the whole pipeline crosses end to end
/// (annotate → `boltffi pack apple` → XCFramework → local SwiftPM dep → `swift test` on the
/// macOS slice) with a single trivial exported function, BEFORE any real wrapper code.
final class SkeletonTests: XCTestCase {
    func testPingCrossesTheFfiBoundary() {
        XCTAssertEqual(ping(input: "hello"), "pong: hello")
    }
}
