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
    public var auto_update_check = true
    public var update_notify = true
    public init() {}
}
public struct UpdatePreferences: Codable {
    public var auto_update_check: Bool
    public var update_notify: Bool
    public init(autoCheck: Bool, notify: Bool) { auto_update_check = autoCheck; update_notify = notify }
}

public extension Client {
    func saveUpdatePreferences(_ preferences: UpdatePreferences, authorized: Bool) throws -> Data {
        if #available(macOS 13, *), authorized, dataDirectory == nil,
           ProcessInfo.processInfo.environment["NETAUTH_CLI"] == nil {
            return try Helper.call("set_update_preferences", payload: JSONEncoder().encode(preferences))
        }
        return try execute(arguments: arguments(for: .updatePreferences) + ["--auto-check", String(preferences.auto_update_check), "--notify", String(preferences.update_notify)], authorized: authorized)
    }
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
    public let auto_update_check: Bool?
    public let update_notify: Bool?
}
public struct DiagnosticReport: Codable {
    public let version: String
    public let state: String
    public let log: String
}

public struct AuthenticationPreference: Codable {
    public let auto_auth: Bool
    public init(enabled: Bool) { auto_auth = enabled }
}
public extension Client {
    func saveAuthenticationPreference(_ enabled: Bool, authorized: Bool) throws -> Data {
        if #available(macOS 13, *), authorized, dataDirectory == nil {
            return try Helper.call("set_preferences", payload: JSONEncoder().encode(AuthenticationPreference(enabled: enabled)))
        }
        return try execute(arguments: arguments(for: .preferences) + ["--auto-auth", String(enabled)], authorized: authorized)
    }
}
