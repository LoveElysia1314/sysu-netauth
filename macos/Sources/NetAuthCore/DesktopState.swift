import Foundation

public enum ConnectionNotice: Equatable {
    case currentState, disconnected, recovered
    public func message(state: String) -> String {
        switch self {
        case .currentState: return state == "authenticated" ? "已认证，互联网连通" : "认证失败，请打开程序查看详情。"
        case .disconnected: return "连接已中断，请检查网络或认证设置。"
        case .recovered: return "连接已恢复，互联网连通。"
        }
    }
}

/// One five-second outage deadline, matching the Windows policy.
public struct StateNotifications {
    private var previous: String?
    private var manual = false
    private var outage: TimeInterval?
    private var warned = false
    public init() {}
    public mutating func connectRequested() { manual = true }
    public mutating func serviceConnectRequested(current: String) {
        manual = true
        if current == "authenticated" || current == "failed" { previous = "authenticating" }
    }
    public mutating func cancelConnectRequest() { manual = false }
    public var deadline: TimeInterval? { warned ? nil : outage.map { $0 + 5 } }
    public mutating func observe(_ state: String, paused: Bool = false, enabled: Bool, now: TimeInterval) -> ConnectionNotice? {
        let before = previous
        previous = state
        if paused { outage = nil; manual = false; warned = false; return nil }
        if state == "authenticated" {
            let since = outage
            outage = nil
            warned = false
            guard let before = before, before != state else { return nil }
            let requested = manual
            manual = false
            guard enabled else { return nil }
            if requested { return .currentState }
            guard let since = since else { return .currentState }
            return now - since >= 5 ? .recovered : nil
        }
        if before == "authenticated" && !manual { outage = now; warned = false }
        if outage == nil, state == "failed", let before = before, before != state {
            manual = false
            return enabled ? .currentState : nil
        }
        return poll(enabled: enabled, now: now)
    }
    public mutating func poll(enabled: Bool, now: TimeInterval) -> ConnectionNotice? {
        guard enabled, !warned, let since = outage, now - since >= 5 else { return nil }
        warned = true
        return .disconnected
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
