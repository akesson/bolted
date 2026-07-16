// M0 smoke: the resolution chain (env → container → path) behaves. The wire-level tests against
// a real syncd arrive with the ViewModel in M2.

import XCTest

@testable import BoltedSyncCore

final class GroupSocketTests: XCTestCase {
    func testSocketPathLandsInsideTheGroupContainer() throws {
        let group = "TESTTEAM00.dev.bolted.os-spike-test"
        let path = try XCTUnwrap(GroupSocket.socketPath(group: group))
        XCTAssertTrue(path.contains("Group Containers"), path)
        XCTAssertTrue(path.hasSuffix("/\(GroupSocket.socketName)"), path)
        XCTAssertTrue(path.contains(group), path)
    }
}
