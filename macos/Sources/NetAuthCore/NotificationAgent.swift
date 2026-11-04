import Foundation
import Darwin

/// A separate instance of the signed application runs without a window or menu-bar item.
/// The per-user lock also lets the UI defer notifications to that instance.
public final class NotificationAgentLease {
    private let descriptor: Int32
    private init(_ descriptor: Int32) { self.descriptor = descriptor }
    deinit { close(descriptor) }
    private static func lockURL(name: String = "notifications") throws -> URL {
        let folder = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                appropriateFor: nil, create: true).appendingPathComponent("SYSUNetAuth")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder.appendingPathComponent(name + ".lock")
    }
    public static func acquire(name: String = "notifications") throws -> NotificationAgentLease? {
        guard ["notifications", "interface"].contains(name) else { throw ClientError.failed("无效锁名称") }
        let fd = open(try lockURL(name: name).path, O_RDWR | O_CREAT | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw ClientError.failed("无法创建通知助手锁") }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else { close(fd); return nil }
        return NotificationAgentLease(fd)
    }
    public static var isRunning: Bool {
        guard let url = try? lockURL() else { return false }
        let fd = open(url.path, O_RDWR | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { return false }
        defer { close(fd) }
        if flock(fd, LOCK_EX | LOCK_NB) == 0 { return false }
        return errno == EWOULDBLOCK
    }
    public static func start(executable: URL) throws {
        guard #available(macOS 13, *) else { return }
        guard !isRunning else { return }
        try launch(executable: executable, arguments: ["--notifications"])
    }
    public static func launch(executable: URL, arguments: [String] = []) throws {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
    }
}
