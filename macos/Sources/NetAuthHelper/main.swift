import Foundation
import NetAuthCore
import Darwin

// This broker never accepts commands, executable paths or alternate service endpoints.
func relay(_ action: String, payload: Data?) throws -> Data {
    let bytes = try Helper.encodeRequest(action, payload: payload)
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else { throw ClientError.failed("无法创建服务连接") }
    defer { close(fd) }
    var timeout = timeval(tv_sec: 5, tv_usec: 0)
    var one: Int32 = 1
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout.size(ofValue: one)))
    var address = sockaddr_un()
    address.sun_family = sa_family_t(AF_UNIX)
    let path = Array("/var/run/sysu-netauth/control.sock".utf8) + [0]
    withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: path) }
    address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
    let connected = withUnsafePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
    }
    guard connected == 0 else { throw ClientError.failed("service unavailable：后台认证服务未运行") }
    func transfer(_ data: inout Data, writing: Bool) throws {
        try data.withUnsafeMutableBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                let count = writing ? Darwin.write(fd, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                                    : Darwin.read(fd, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw ClientError.failed("服务连接断开或超时") }
                offset += count
            }
        }
    }
    var length = UInt32(bytes.count).bigEndian
    var outgoing = withUnsafeBytes(of: &length) { Data($0) } + bytes
    try transfer(&outgoing, writing: true)
    var prefix = Data(count: 4)
    try transfer(&prefix, writing: false)
    let size = prefix.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    guard size > 0 && size <= 65536 else { throw ClientError.failed("无效服务响应长度") }
    var response = Data(count: Int(size))
    try transfer(&response, writing: false)
    guard let object = try JSONSerialization.jsonObject(with: response) as? [String: Any],
          object["version"] as? Int == 1, object["id"] as? Int == 1 else { throw ClientError.failed("不兼容的服务响应") }
    guard object["ok"] as? Bool == true else { throw ClientError.failed(object["error"] as? String ?? "服务错误") }
    return try JSONSerialization.data(withJSONObject: object["data"] ?? NSNull(), options: [.fragmentsAllowed])
}

final class Handler: NSObject, HelperProtocol {
    static let slots = DispatchSemaphore(value: 8)
    func request(_ action: String, payload: Data?, reply: @escaping (Data?, String?) -> Void) {
        guard Self.slots.wait(timeout: .now()) == .success else { reply(nil, "helper 忙，请稍后重试"); return }
        DispatchQueue.global(qos: .userInitiated).async {
            defer { Self.slots.signal() }
            do { reply(try relay(action, payload: payload), nil) }
            catch { reply(nil, error.localizedDescription) }
        }
    }
}
@available(macOS 13, *)
final class Delegate: NSObject, NSXPCListenerDelegate {
    let uid: uid_t
    let requirement: String
    init(uid: uid_t, requirement: String) { self.uid = uid; self.requirement = requirement }
    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        guard connection.effectiveUserIdentifier == uid else { return false }
        connection.setCodeSigningRequirement(requirement)
        connection.exportedInterface = NSXPCInterface(with: HelperProtocol.self)
        connection.exportedObject = Handler()
        connection.resume()
        return true
    }
}
if #available(macOS 13, *) {
    do {
        guard geteuid() == 0 else { throw ClientError.failed("helper 必须由系统运行") }
        let path = "/Library/PrivilegedHelperTools/sysu_netauth_gui_policy.json"
        let attributes = try FileManager.default.attributesOfItem(atPath: path)
        guard attributes[.type] as? FileAttributeType == .typeRegular,
              attributes[.ownerAccountID] as? Int == 0,
              (attributes[.posixPermissions] as? Int ?? 0) & 0o077 == 0 else { throw ClientError.failed("helper 策略权限不安全") }
        guard let policy = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: Any] else { throw ClientError.failed("无效 helper 策略") }
        guard let uid = policy["uid"] as? UInt32, uid > 0, let hash = policy["cdhash"] as? String else { throw ClientError.failed("无效 helper 策略") }
        let delegate = Delegate(uid: uid, requirement: try Helper.requirement(hash))
        let listener = NSXPCListener(machServiceName: Helper.service)
        listener.delegate = delegate
        listener.setConnectionCodeSigningRequirement(delegate.requirement)
        listener.resume()
        withExtendedLifetime(delegate) { RunLoop.current.run() }
    } catch { fputs("helper startup failed: \(error.localizedDescription)\n", stderr); exit(1) }
} else { exit(1) }
