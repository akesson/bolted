import XCTest

/// M2 go/no-go: proves `xcodebuild test` can launch the app and attach the accessibility hierarchy
/// on this machine (the earlier `-25211` automation denial was the risk). If this is green, the
/// full item-2..6 suite is viable; if it dies on permissions, stop and report.
final class SmokeUITest: XCTestCase {
    func testAppLaunchesWithWindow() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 15), "no window appeared")
        XCTAssertTrue(
            app.staticTexts["Edit profile"].waitForExistence(timeout: 5),
            "editor title 'Edit profile' not found — accessibility hierarchy not attached"
        )
        app.terminate()
    }
}
