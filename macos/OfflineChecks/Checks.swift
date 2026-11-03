import Foundation
@main struct OfflineCheck {
 static func main() throws {
  let client = Client(executable: URL(fileURLWithPath: "/unused"), dataDirectory: "/tmp/a b;$(id)")
  precondition(client.arguments(for: .authenticate) == ["--json", "--data-dir", "/tmp/a b;$(id)", "authenticate"])
  let json = Data(#"{"state":"idle","message":"mock","internet":false,"adapter":null,"suspended":false,"manual_disconnect":false,"adapters":[]}"#.utf8)
  let snapshot = try JSONDecoder().decode(Snapshot.self, from: json)
  precondition(snapshot.state == "idle")
  let adapterJSON = Data(#"[{"name":"en5","ipv4":"192.0.2.1","up":true,"ethernet":true,"id":"00:01:02:03:04:05"}]"#.utf8)
  let adapters = try JSONDecoder().decode([Adapter].self, from: adapterJSON)
  precondition(adapters.count == 1)
  let file = URL(fileURLWithPath: "/private/tmp/netauth-mock-" + UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: file) }
  func mock(_ body: String) throws -> Client {
   try Data(("#!/bin/sh\n" + body).utf8).write(to: file)
   try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: file.path)
   return Client(executable: file)
  }
  let success = try mock("printf '%s' '" + String(data: json, encoding: .utf8)! + "'\n")
  let output = try success.run(.status)
  precondition(output == json)
  let failed = try mock("echo 'service unavailable' >&2\nexit 1\n")
  do { _ = try failed.run(.status); fatalError("expected error") }
  catch { precondition(error.localizedDescription.contains("service unavailable")) }
  let blocked = try mock("while :; do :; done\n")
  let start = Date()
  do { _ = try blocked.run(.status, timeout: 0.1); fatalError("expected timeout") }
  catch { precondition(Date().timeIntervalSince(start) < 3) }
  do { _ = try Client(executable: URL(fileURLWithPath: "/nonexistent/netauth")).run(.status); fatalError("expected missing CLI") }
  catch { }
  let resolved = Client.resolveExecutable(environment: ["NETAUTH_CLI": "/tmp/custom cli"])
  precondition(resolved.path == "/tmp/custom cli")
  let literal = "a b'\\\";$(id)"
  let quoted = try Client(executable: URL(fileURLWithPath: "/bin/sh"))
      .execute(arguments: ["-c", "printf %s " + Client.shellQuote(literal)])
  precondition(String(data: quoted, encoding: .utf8) == literal)
  let settings = Settings()
  let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as! [String: Any]
  precondition(encoded["retry_interval"] as? Int == 60)
  func rejects(_ action: String, _ payload: Data?) {
    do { _ = try Helper.encodeRequest(action, payload: payload); fatalError("request accepted") } catch { }
  }
  rejects("exec", nil)
  rejects("status", Data("{}".utf8))
  rejects("set_config", nil)
  rejects("set_config", Data("[]".utf8))
  rejects("set_config", Data(repeating: 0, count: 60001))
  let request = try JSONSerialization.jsonObject(with: Helper.encodeRequest("set_config", payload: JSONEncoder().encode(settings))) as! [String: Any]
  precondition(request["action"] as? String == "set_config")
  precondition((request["config"] as? [String: Any])?["retry_interval"] as? Int == 60)
  do { _ = try Helper.requirement("bad\" or true"); fatalError("invalid hash accepted") } catch { }
  rejects("connect", nil)
  let connectRequest = try JSONSerialization.jsonObject(with: Helper.encodeRequest("connect", payload: JSONEncoder().encode(settings))) as! [String: Any]
  precondition(connectRequest["action"] as? String == "connect")
  var notices = StateNotifications()
  precondition(notices.observe("authenticated", enabled: true, now: 0) == nil)
  precondition(notices.observe("idle", enabled: true, now: 1) == nil)
  precondition(notices.observe("authenticated", enabled: true, now: 5) == nil)
  precondition(notices.observe("idle", enabled: true, now: 10) == nil)
  precondition(notices.observe("authenticating", enabled: true, now: 12) == nil)
  precondition(notices.deadline == 15)
  precondition(notices.poll(enabled: true, now: 14) == nil)
  precondition(notices.poll(enabled: true, now: 15) == .disconnected)
  precondition(notices.poll(enabled: true, now: 16) == nil)
  precondition(notices.observe("authenticated", enabled: true, now: 17) == .recovered)
  notices.connectRequested()
  precondition(notices.observe("authenticating", enabled: true, now: 18) == nil)
  precondition(notices.observe("authenticated", enabled: true, now: 19) == .currentState)
  precondition(notices.observe("idle", paused: true, enabled: true, now: 20) == nil)
  precondition(notices.poll(enabled: true, now: 30) == nil)
  precondition(!DesktopState.hideAtLaunch(loginLaunch: false, hideEnabled: true))
  precondition(!DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: false))
  precondition(DesktopState.hideAtLaunch(loginLaunch: true, hideEnabled: true))
  precondition(DesktopState.symbol("failed") != DesktopState.symbol("idle"))
  precondition(StatusMark.resolve(state: "idle") == .ready)
  precondition(StatusMark.resolve(state: "idle", message: "已断开") == .disconnected)
  precondition(StatusMark.resolve(state: "stopped") == .disconnected)
  precondition(StatusMark.resolve(state: "authenticating") == .authenticating)
  precondition(StatusMark.resolve(state: "authenticated") == .authenticated)
  precondition(StatusMark.resolve(state: "failed") == .failed)
  precondition(StatusMark.resolve(state: "unknown") == .unavailable)
  precondition(StatusMark.authenticated.colorName == "green")
  precondition(StatusMark.failed.colorName == "red")
  precondition(Set(StatusMark.allCases.map(\.label)).count == 6)
  print("Offline checks passed; no service or network accessed")
 }
}
