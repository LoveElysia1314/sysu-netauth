import XCTest
@testable import NetAuthCore

final class ClientTests: XCTestCase {
    func testResolveOverride() {
        XCTAssertEqual(Client.resolveExecutable(environment: ["NETAUTH_CLI": "/tmp/custom cli"]).path, "/tmp/custom cli")
    }

    func testShellQuoteRoundTrip() throws {
        let value = "a b'\\\";$(id)"
        let output = try Client(executable: URL(fileURLWithPath: "/bin/sh"))
            .execute(arguments: ["-c", "printf %s " + Client.shellQuote(value)])
        XCTAssertEqual(String(data: output, encoding: .utf8), value)
    }

    func testSettingsJSON() throws {
        let data = try JSONEncoder().encode(Settings())
        let value = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        XCTAssertEqual(value["retry_interval"] as? Int, 60)
        XCTAssertEqual(value["iface_mode"] as? String, "auto")
    }

    func testArgumentsKeepPathsLiteral() {
        let client = Client(executable: URL(fileURLWithPath: "/unused"), dataDirectory: "/tmp/a b;$(id)")
        XCTAssertEqual(client.arguments(for: .authenticate), ["--json", "--data-dir", "/tmp/a b;$(id)", "authenticate"])
    }

    func testSnapshotContract() throws {
        let data = Data(#"{"state":"idle","message":"ready","internet":false,"adapter":null,"adapters":[],"update":{}}"#.utf8)
        let snapshot = try JSONDecoder().decode(Snapshot.self, from: data)
        XCTAssertEqual(snapshot.state, "idle")
        XCTAssertNil(snapshot.adapter)
        XCTAssertFalse(snapshot.internet)
        XCTAssertThrowsError(try JSONDecoder().decode(Snapshot.self, from: Data("{}".utf8)))
    }

    func testAdapterContract() throws {
        let data = Data(#"[{"name":"en5","id":"en5","index":5,"mac":[0,1,2,3,4,5],"up":true,"ethernet":true,"score":100,"ipv4":"192.0.2.1","gateway":"","dns":"","driver":""}]"#.utf8)
        let adapters = try JSONDecoder().decode([Adapter].self, from: data)
        XCTAssertEqual(adapters.first?.name, "en5")
    }

    func withMock(_ body: String, test: (Client) throws -> Void) throws {
        let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try Data(("#!/bin/sh\n" + body).utf8).write(to: path)
        defer { try? FileManager.default.removeItem(at: path) }
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: path.path)
        try test(Client(executable: path))
    }

    func testMockCLI() throws {
        try withMock("[ \"$1\" = --json ] && [ \"$2\" = status ] || exit 2\nprintf '{\"state\":\"idle\",\"message\":\"mock\",\"internet\":false,\"adapter\":null}'\n") { client in
            let snapshot = try JSONDecoder().decode(Snapshot.self, from: client.run(.status))
            XCTAssertEqual(snapshot.message, "mock")
        }
    }

    func testServiceError() throws {
        try withMock("echo 'service unavailable' >&2\nexit 1\n") { client in
            XCTAssertThrowsError(try client.run(.status)) { error in
                XCTAssertTrue(error.localizedDescription.contains("service unavailable"))
            }
        }
    }

    func testTimeout() throws {
        try withMock("while :; do :; done\n") { client in
            let start = Date()
            XCTAssertThrowsError(try client.run(.status, timeout: 0.1))
            XCTAssertLessThan(Date().timeIntervalSince(start), 3)
        }
    }

    func testMissingExecutable() {
        XCTAssertThrowsError(try Client(executable: URL(fileURLWithPath: "/nonexistent/netauth")).run(.status))
    }
}
