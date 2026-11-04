import XCTest
@testable import NetAuthCore

final class StatusMarkTests: XCTestCase {
    func testRedrawIdentityChangesOnlyWhenVisibleArtworkChanges() {
        let initial = StatusIconRenderKey(mark: .ready, colored: false, appearance: "aqua")
        for _ in 0..<10000 {
            XCTAssertEqual(initial, StatusIconRenderKey(mark: .ready, colored: false, appearance: "aqua"))
        }
        XCTAssertNotEqual(initial, StatusIconRenderKey(mark: .failed, colored: false, appearance: "aqua"))
        XCTAssertNotEqual(initial, StatusIconRenderKey(mark: .ready, colored: true, appearance: "aqua"))
        XCTAssertNotEqual(initial, StatusIconRenderKey(mark: .ready, colored: false, appearance: "darkAqua"))
    }
    func testStateAndWindowsColorMapping() {
        let cases: [(String, StatusMark, String)] = [
            ("idle", .ready, "blue"), ("authenticating", .authenticating, "orange"),
            ("authenticated", .authenticated, "green"), ("failed", .failed, "red"),
            ("stopped", .disconnected, "gray"), ("unavailable", .unavailable, "gray")
        ]
        for (state, mark, color) in cases {
            XCTAssertEqual(StatusMark.resolve(state: state), mark)
            XCTAssertEqual(mark.colorName, color)
        }
    }
    func testManualDisconnectAndUnknownStateRemainDistinct() {
        XCTAssertEqual(StatusMark.resolve(state: "idle", message: "已断开"), .disconnected)
        XCTAssertEqual(StatusMark.resolve(state: "idle", message: "待命"), .ready)
        XCTAssertEqual(StatusMark.resolve(state: "future_state"), .unavailable)
        XCTAssertEqual(Set(StatusMark.allCases.map(\.label)).count, 6)
    }
}
