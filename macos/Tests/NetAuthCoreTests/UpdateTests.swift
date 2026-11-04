import XCTest
@testable import NetAuthCore

final class UpdateTests: XCTestCase {
    func testExplicitConnectionResultIsReportedWhenPollingMissesTheHandshake() {
        var gate = StateNotifications()
        XCTAssertNil(gate.observe("authenticated", enabled: true, now: 0))
        gate.serviceConnectRequested(current: "authenticated")
        XCTAssertEqual(gate.observe("authenticated", enabled: true, now: 1), .currentState)
        XCTAssertNil(gate.observe("authenticated", enabled: true, now: 2))
    }
    func update(available: Bool = true, checking: Bool = false) throws -> ReleaseUpdate {
        try JSONDecoder().decode(ReleaseUpdate.self, from: Data("""
        {"latest_version":"99.0.0","release_url":"https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v99.0.0","summary":"example","available":\(available),"checking":\(checking),"checked_at":1,"error":""}
        """.utf8))
    }
    func testOnlyAvailableUnseenUnignoredReleasesNotify() throws {
        let empty = try JSONDecoder().decode(ReleaseUpdate.self, from: Data("{}".utf8))
        XCTAssertFalse(empty.shouldNotify(ignored: nil, notified: nil))
        let release = try update()
        XCTAssertTrue(release.shouldNotify(ignored: nil, notified: nil))
        XCTAssertFalse(release.shouldNotify(ignored: "99.0.0", notified: nil))
        XCTAssertFalse(release.shouldNotify(ignored: nil, notified: "99.0.0"))
        XCTAssertFalse(try update(available: false).shouldNotify(ignored: nil, notified: nil))
        XCTAssertFalse(try update(checking: true).shouldNotify(ignored: nil, notified: nil))
    }
    func testUntrustedReleaseLinksAreRejected() {
        for value in ["http://github.com/a", "https://github.com.evil/a", "https://user@github.com/a", "https://github.com:80/a", "https://github.com\\@evil/a"] {
            XCTAssertNil(ReleaseUpdate.trustedURL(value), value)
        }
        XCTAssertNotNil(ReleaseUpdate.trustedURL("https://gitee.com/a/b/releases"))
    }
    func testUpdateCheckUsesOnlyTheFixedHelperOperation() throws {
        let client = Client(executable: URL(fileURLWithPath: "/unused"))
        XCTAssertEqual(client.arguments(for: .checkUpdate), ["--json", "check-update"])
        let request = try JSONSerialization.jsonObject(with: Helper.encodeRequest("check_update", payload: nil)) as! [String: Any]
        XCTAssertEqual(request["action"] as? String, "check_update")
        XCTAssertThrowsError(try Helper.encodeRequest("check_update", payload: Data("{}".utf8)))
    }
    func testAssistantHasOneOwnerAndTheUICanDetectIt() throws {
        var lease = try NotificationAgentLease.acquire()
        XCTAssertNotNil(lease)
        try withExtendedLifetime(lease) {
            XCTAssertTrue(NotificationAgentLease.isRunning)
            XCTAssertNil(try NotificationAgentLease.acquire())
        }
        lease = nil
        XCTAssertFalse(NotificationAgentLease.isRunning)
    }
}
