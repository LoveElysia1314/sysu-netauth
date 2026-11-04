import Foundation

public struct Adapter: Decodable, Equatable {
    public let id: String
    public let name: String
    public var ipv4: String
    public let ethernet: Bool
    public var up: Bool
    public let driver: String?
    public var gateway: String?
    public var dns: String?
    public let mac: [UInt8]?
}

public struct Snapshot: Decodable {
    public let state: String
    public let message: String
    public let internet: Bool
    public let adapter: Adapter?
    public let adapters: [Adapter]
    public let suspended: Bool
    public let manual_disconnect: Bool
    public let connection_request: UInt64?
    public let update: ReleaseUpdate?
    public let update_notify: Bool?
}

public enum AdapterSelection {
    public static func restore(_ all: [Adapter], preferredID: String) -> [Adapter] {
        guard !preferredID.isEmpty, !all.contains(where: { $0.id == preferredID }) else { return all }
        return all + [Adapter(id: preferredID, name: "已保存网卡", ipv4: "", ethernet: true, up: false,
            driver: nil, gateway: nil, dns: nil, mac: nil)]
    }
    public static func refresh(_ all: [Adapter], selected: Adapter?) -> [Adapter] {
        var items = all.filter(\.ethernet)
        if var missing = selected, !items.contains(where: { $0.id == missing.id }) {
            missing.up = false
            missing.ipv4 = ""
            missing.gateway = nil
            missing.dns = nil
            items.append(missing)
        }
        return items
    }
}

public enum Action: String {
    case interfaces, status, authenticate, logoff, config, connect, preferences, diagnostics
    case checkUpdate = "check-update"
    case updatePreferences = "update-preferences"
}

public enum ClientError: LocalizedError {
    case failed(String)
    public var errorDescription: String? {
        switch self { case .failed(let message): return message }
    }
}

/// Runs only fixed CLI subcommands. No shell or credentials in arguments.
public struct Client {
    public let executable: URL
    public let dataDirectory: String?

    public init(executable: URL, dataDirectory: String? = nil) {
        self.executable = executable
        self.dataDirectory = dataDirectory
    }

    public func arguments(for action: Action) -> [String] {
        var args = ["--json"]
        if let directory = dataDirectory { args += ["--data-dir", directory] }
        return args + [action.rawValue]
    }

    /// Call on a worker queue. A file captures output without pipe-buffer deadlocks.
    public func run(_ action: Action, timeout: TimeInterval = 15, authorized: Bool = false) throws -> Data {
        if #available(macOS 13, *), authorized, dataDirectory == nil,
           ProcessInfo.processInfo.environment["NETAUTH_CLI"] == nil,
           [.status, .authenticate, .logoff, .preferences, .diagnostics, .checkUpdate].contains(action) {
            return try Helper.call(action == .checkUpdate ? "check_update" : action.rawValue)
        }
        return try execute(arguments: arguments(for: action), timeout: timeout, authorized: authorized)
    }

    public func execute(arguments: [String], timeout: TimeInterval = 15, authorized: Bool = false) throws -> Data {
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw ClientError.failed("未找到认证组件，请重新构建完整应用或安装 CLI。路径：\(executable.path)")
        }
        let output = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        guard FileManager.default.createFile(atPath: output.path, contents: nil,
                                             attributes: [.posixPermissions: 0o600]) else {
            throw ClientError.failed("无法创建临时输出文件")
        }
        defer { try? FileManager.default.removeItem(at: output) }
        let handle = try FileHandle(forWritingTo: output)
        defer { try? handle.close() }
        let process = Process()
        if authorized {
            let command = ([executable.path] + arguments).map(Self.shellQuote).joined(separator: " ")
            // Encode a literal AppleScript string; no credentials enter the command line.
            let literal = command.replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"")
            process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
            process.arguments = ["-e", "do shell script \"\(literal)\" with administrator privileges"]
        } else {
            process.executableURL = executable
            process.arguments = arguments
        }
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = handle
        process.standardError = handle
        try process.run()
        let deadline = Date().addingTimeInterval(authorized ? max(timeout, 120) : timeout)
        while process.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
        if process.isRunning {
            process.terminate()
            // A child that ignores SIGTERM must not leave the UI permanently busy.
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            throw ClientError.failed("命令超时，请检查后台服务")
        }
        process.waitUntilExit()
        let data = try Data(contentsOf: output)
        guard process.terminationStatus == 0 else {
            throw ClientError.failed(String(data: data, encoding: .utf8) ?? "CLI 执行失败")
        }
        return data
    }
}
