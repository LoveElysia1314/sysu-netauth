import XCTest
@testable import NetAuthCore

final class UpdateTests: XCTestCase {
    func testHelperUpdatePreferencesHaveOnlyTwoBooleanFields() throws {
        let payload = try JSONEncoder().encode(UpdatePreferences(autoCheck: false, notify: true))
        let data = try Helper.encodeRequest("set_update_preferences", payload: payload)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(object["auto_update_check"] as? Bool, false)
        XCTAssertEqual(object["update_notify"] as? Bool, true)
        XCTAssertNil(object["config"])
        for invalid in [#"{"auto_update_check":false}"#, #"{"auto_update_check":0,"update_notify":true}"#, #"{"auto_update_check":true,"update_notify":true,"password":"injection"}"#] {
            XCTAssertThrowsError(try Helper.encodeRequest("set_update_preferences", payload: Data(invalid.utf8)))
        }
        XCTAssertThrowsError(try Helper.encodeRequest("set_update_preferences", payload: nil))
    }
    func testUpdatePreferencesEncodeIndependentOptOut() throws {
        var settings = Settings()
        XCTAssertTrue(settings.auto_update_check && settings.update_notify)
        settings.auto_update_check = false
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as? [String: Any])
        XCTAssertEqual(object["auto_update_check"] as? Bool, false)
        XCTAssertEqual(object["update_notify"] as? Bool, true)
        let preferences = try JSONDecoder().decode(Preferences.self, from: Data(#"{"auto_auth":true,"retry_interval":60,"iface_mode":"auto","iface":"","auto_update_check":false,"update_notify":false}"#.utf8))
        XCTAssertEqual(preferences.auto_update_check, false)
        XCTAssertEqual(preferences.update_notify, false)
    }
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
    func testChangelogPrefersUnignoredNewReleaseAndOtherwiseCurrentVersion() throws {
        let current = "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0"
        let latest = "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v99.0.0"
        XCTAssertEqual(try update().changelogURL(currentVersion: "0.7.0", ignored: nil)?.absoluteString, latest)
        XCTAssertEqual(try update().changelogURL(currentVersion: "0.7.0", ignored: "99.0.0")?.absoluteString, current)
        XCTAssertEqual(try update(available: false).changelogURL(currentVersion: "0.7.0", ignored: nil)?.absoluteString, current)
        for link in ["", "http://github.com/a", "https://evil.example/a"] {
            let data = try JSONSerialization.data(withJSONObject: ["available": true, "latest_version": "99.0.0", "release_url": link])
            let release = try JSONDecoder().decode(ReleaseUpdate.self, from: data)
            XCTAssertEqual(release.changelogURL(currentVersion: "0.7.0", ignored: nil)?.absoluteString, current)
        }
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
