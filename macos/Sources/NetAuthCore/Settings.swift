import Foundation

public struct Settings: Codable {
    public var username = ""
    public var password = ""
    public var iface = ""
    public var iface_mode = "auto"
    public var auto_auth = true
    public var retry_interval = 60
    public var service_mode = true
    public var launch_gui_on_login = false
    public var hide_window_on_login = true
    public var desktop_notify = false
    public init() {}
}

public extension Client {
    static func resolveExecutable(environment: [String: String] = ProcessInfo.processInfo.environment,
                                  bundle: Bundle = .main) -> URL {
        if let path = environment["NETAUTH_CLI"] { return URL(fileURLWithPath: path) }
        if let resources = bundle.resourceURL {
            let bundled = resources.appendingPathComponent("sysu_netauth")
            if FileManager.default.isExecutableFile(atPath: bundled.path) { return bundled }
        }
        return URL(fileURLWithPath: "/usr/local/bin/sysu_netauth")
    }

    static func shellQuote(_ text: String) -> String {
        "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    func save(_ settings: Settings, authorized: Bool, connect: Bool = false) throws -> Data {
        if #available(macOS 13, *), authorized, dataDirectory == nil,
           ProcessInfo.processInfo.environment["NETAUTH_CLI"] == nil {
            return try Helper.call(connect ? "connect" : "set_config", payload: JSONEncoder().encode(settings))
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                               attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("settings.json")
        try JSONEncoder().encode(settings).write(to: file, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        let arguments = connect ? self.arguments(for: .connect) + [file.path]
                                : self.arguments(for: .config) + ["import", file.path]
        return try execute(arguments: arguments,
                           authorized: authorized)
    }
}

public struct Preferences: Decodable {
    public let auto_auth: Bool
    public let retry_interval: Int
    public let iface_mode: String
    public let iface: String
}
public struct DiagnosticReport: Codable {
    public let version: String
    public let state: String
    public let log: String
}
