import Foundation

/// Mirrors the Windows notification policy, using monotonic elapsed time.
public struct StateNotifications {
    private var previous: String?
    private var last: [String: TimeInterval] = [:]
    public init() {}
    public mutating func observe(_ state: String, enabled: Bool, now: TimeInterval) -> Bool {
        defer { previous = state }
        guard enabled, let previous = previous, previous != state else { return false }
        let cooldown: TimeInterval
        switch state {
        case "authenticated": cooldown = 180
        case "failed": cooldown = 300
        default: return false
        }
        if let time = last[state], now - time < cooldown { return false }
        last[state] = now
        return true
    }
}

public enum DesktopState {
    public static func label(_ state: String) -> String {
        ["idle": "待机", "authenticating": "正在认证", "authenticated": "已认证", "failed": "认证失败", "stopped": "已停止"][state] ?? "状态未知"
    }
    public static func symbol(_ state: String) -> String {
        switch state {
        case "authenticated": return "network.badge.shield.half.filled"
        case "authenticating": return "arrow.triangle.2.circlepath"
        case "failed", "unavailable": return "exclamationmark.triangle"
        default: return "network"
        }
    }
    public static func hideAtLaunch(loginLaunch: Bool, hideEnabled: Bool) -> Bool {
        loginLaunch && hideEnabled
    }
}
