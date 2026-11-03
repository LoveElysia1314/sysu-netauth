import Foundation
import Security

@objc public protocol HelperProtocol {
    func request(_ action: String, payload: Data?, reply: @escaping (Data?, String?) -> Void)
}

public enum Helper {
    public static let service = "io.github.peterrobot.sysu-netauth.gui-helper"
    public static func hash(of url: URL) throws -> String {
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess, let code = code,
              SecStaticCodeCheckValidity(code, [], nil) == errSecSuccess else {
            throw ClientError.failed("组件签名无效")
        }
        var info: CFDictionary?
        guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
              let data = (info as? [String: Any])?[kSecCodeInfoUnique as String] as? Data else {
            throw ClientError.failed("无法读取组件签名")
        }
        return data.map { String(format: "%02x", $0) }.joined()
    }
    public static func requirement(_ hash: String) throws -> String {
        guard hash.count == 40, hash.allSatisfy({ $0.isHexDigit && $0.isASCII }) else {
            throw ClientError.failed("无效的 helper 签名配置")
        }
        return "cdhash H\"\(hash)\""
    }
    public static func encodeRequest(_ action: String, payload: Data?) throws -> Data {
        guard ["status", "authenticate", "logoff", "set_config", "connect"].contains(action),
              (payload?.count ?? 0) <= 60000 else { throw ClientError.failed("不允许的 helper 请求") }
        var request: [String: Any] = ["version": 1, "id": 1, "action": action]
        if action == "set_config" || action == "connect" {
            guard let payload = payload,
                  let config = try JSONSerialization.jsonObject(with: payload) as? [String: Any] else { throw ClientError.failed("无效配置") }
            request["config"] = config
        } else if payload != nil { throw ClientError.failed("该操作不接受参数") }
        let bytes = try JSONSerialization.data(withJSONObject: request)
        guard bytes.count <= 65536 else { throw ClientError.failed("请求过大") }
        return bytes
    }
    @available(macOS 13, *)
    public static func call(_ action: String, payload: Data? = nil) throws -> Data {
        _ = try encodeRequest(action, payload: payload)
        guard let binary = Bundle.main.resourceURL?.appendingPathComponent("SYSUNetAuthHelper") else {
            throw ClientError.failed("应用缺少 helper，请重新构建")
        }
        let connection = NSXPCConnection(machServiceName: service, options: .privileged)
        connection.remoteObjectInterface = NSXPCInterface(with: HelperProtocol.self)
        connection.setCodeSigningRequirement(try requirement(hash(of: binary)))
        let semaphore = DispatchSemaphore(value: 0)
        let lock = NSLock()
        var result: Result<Data, Error>?
        func finish(_ value: Result<Data, Error>) {
            lock.lock(); defer { lock.unlock() }
            if result == nil { result = value; semaphore.signal() }
        }
        connection.invalidationHandler = { finish(.failure(ClientError.failed("helper 不可用或签名不匹配，请重新安装后台服务"))) }
        connection.interruptionHandler = { finish(.failure(ClientError.failed("helper 连接中断，请重试"))) }
        connection.resume()
        defer { connection.invalidate() }
        guard let proxy = connection.remoteObjectProxyWithErrorHandler({ finish(.failure($0)) }) as? HelperProtocol else {
            throw ClientError.failed("无法连接 helper")
        }
        proxy.request(action, payload: payload) { data, error in
            if let error = error { finish(.failure(ClientError.failed(error))) }
            else if let data = data { finish(.success(data)) }
            else { finish(.failure(ClientError.failed("helper 返回无效数据"))) }
        }
        guard semaphore.wait(timeout: .now() + 15) == .success else { throw ClientError.failed("helper 请求超时") }
        lock.lock(); defer { lock.unlock() }
        return try result!.get()
    }
}
