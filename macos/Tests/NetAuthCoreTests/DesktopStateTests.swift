import XCTest
@testable import NetAuthCore

final class DesktopStateTests: XCTestCase {
    func testShortOutagesStaySilentWithoutCooldownRules() {
        var gate = StateNotifications()
        XCTAssertNil(gate.observe("authenticated", enabled: true, now: 0))
        for time in [3600.0, 3610.0, 3620.0] {
            XCTAssertNil(gate.observe("idle", enabled: true, now: time))
            XCTAssertNil(gate.observe("authenticating", enabled: true, now: time + 1))
            XCTAssertNil(gate.observe("authenticated", enabled: true, now: time + 4))
            XCTAssertNil(gate.deadline)
        }
    }
    func testLongOutageHasOneDeadlineAndOneRecovery() {
        var gate = StateNotifications()
        _ = gate.observe("authenticated", enabled: true, now: 0)
        XCTAssertNil(gate.observe("idle", enabled: true, now: 10))
        XCTAssertNil(gate.observe("failed", enabled: true, now: 12))
        XCTAssertNil(gate.observe("authenticating", enabled: true, now: 14))
        XCTAssertEqual(gate.deadline, 15)
        XCTAssertNil(gate.poll(enabled: true, now: 14))
        XCTAssertEqual(gate.poll(enabled: true, now: 15), .disconnected)
        XCTAssertNil(gate.poll(enabled: true, now: 30))
        XCTAssertEqual(gate.observe("authenticated", enabled: true, now: 31), .recovered)
        XCTAssertNil(gate.observe("authenticated", enabled: true, now: 32))
    }
    func testManualConnectionAndInitialFailureAreVisible() {
        var gate = StateNotifications()
        XCTAssertNil(gate.observe("idle", enabled: true, now: 0))
        gate.connectRequested()
        XCTAssertNil(gate.observe("authenticating", enabled: true, now: 1))
        XCTAssertNil(gate.poll(enabled: true, now: 10))
        XCTAssertEqual(gate.observe("authenticated", enabled: true, now: 11), .currentState)
        gate.connectRequested()
        XCTAssertNil(gate.observe("authenticating", enabled: true, now: 12))
        XCTAssertEqual(gate.observe("failed", enabled: true, now: 13), .currentState)
    }
    func testIntentionalDisconnectAndDisabledNotificationsDoNotLeakWarnings() {
        var gate = StateNotifications()
        _ = gate.observe("authenticated", enabled: true, now: 0)
        XCTAssertNil(gate.observe("idle", paused: true, enabled: true, now: 1))
        XCTAssertNil(gate.poll(enabled: true, now: 20))
        _ = gate.observe("authenticated", enabled: false, now: 30)
        _ = gate.observe("idle", enabled: false, now: 31)
        XCTAssertNil(gate.poll(enabled: false, now: 40))
        XCTAssertEqual(gate.poll(enabled: true, now: 41), .disconnected)
        XCTAssertNil(gate.observe("authenticated", enabled: false, now: 42))
        XCTAssertNil(gate.observe("authenticated", enabled: true, now: 43))
    }
    func testHelperConnectRequiresSettingsAndRejectsUnexpectedParameters() throws {
        XCTAssertThrowsError(try Helper.encodeRequest("connect", payload: nil))
        XCTAssertThrowsError(try Helper.encodeRequest("status", payload: Data("{}".utf8)))
        let data = try Helper.encodeRequest("connect", payload: JSONEncoder().encode(Settings()))
        let request = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        XCTAssertEqual(request["action"] as? String, "connect")
        XCTAssertNotNil(request["config"])
    }
    func testHideOnlyForLoginLaunch() {
        XCTAssertFalse(DesktopState.hideAtLaunch(loginLaunch: false, hideEnabled: true))
        XCTAssertFalse(DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: false))
        XCTAssertTrue(DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: true))
    }
}
