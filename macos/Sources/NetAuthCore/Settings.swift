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
    public var desktop_notify = true
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

    func save(_ settings: Settings, authorized: Bool) throws -> Data {
        if #available(macOS 13, *), authorized, dataDirectory == nil,
           ProcessInfo.processInfo.environment["NETAUTH_CLI"] == nil {
            return try Helper.call("set_config", payload: JSONEncoder().encode(settings))
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                               attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("settings.json")
        try JSONEncoder().encode(settings).write(to: file, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        return try execute(arguments: arguments(for: .config) + ["import", file.path],
                           authorized: authorized)
    }
}
