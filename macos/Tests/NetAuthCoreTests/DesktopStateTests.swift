import XCTest
@testable import NetAuthCore

final class DesktopStateTests: XCTestCase {
    func testIndependentNotificationCooldowns() {
        var gate = StateNotifications()
        XCTAssertFalse(gate.observe("idle", enabled: true, now: 0))
        XCTAssertTrue(gate.observe("authenticated", enabled: true, now: 1))
        XCTAssertTrue(gate.observe("failed", enabled: true, now: 2))
        XCTAssertFalse(gate.observe("authenticated", enabled: true, now: 3))
        XCTAssertFalse(gate.observe("failed", enabled: true, now: 4))
        XCTAssertTrue(gate.observe("authenticated", enabled: true, now: 181))
        XCTAssertFalse(gate.observe("failed", enabled: true, now: 301))
        _ = gate.observe("idle", enabled: true, now: 302)
        XCTAssertTrue(gate.observe("failed", enabled: true, now: 303))
    }
    func testInitialRepeatedAndDisabledSnapshotsDoNotNotify() {
        var gate = StateNotifications()
        XCTAssertFalse(gate.observe("authenticated", enabled: true, now: 0))
        XCTAssertFalse(gate.observe("authenticated", enabled: true, now: 1))
        XCTAssertFalse(gate.observe("failed", enabled: false, now: 2))
        XCTAssertFalse(gate.observe("failed", enabled: true, now: 3))
        XCTAssertTrue(gate.observe("authenticated", enabled: true, now: 4))
    }
    func testHideOnlyForLoginLaunch() {
        XCTAssertFalse(DesktopState.hideAtLaunch(loginLaunch: false, hideEnabled: true))
        XCTAssertFalse(DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: false))
        XCTAssertTrue(DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: true))
    }
}
