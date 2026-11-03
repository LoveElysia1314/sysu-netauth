import Foundation

public enum StatusMark: String, CaseIterable {
    case ready, authenticating, authenticated, failed, disconnected, unavailable
    public static func resolve(state: String, message: String = "") -> StatusMark {
        switch state {
        case "authenticated": return .authenticated
        case "authenticating": return .authenticating
        case "failed": return .failed
        case "stopped": return .disconnected
        case "idle": return message == "已断开" ? .disconnected : .ready
        default: return .unavailable
        }
    }
    public var label: String {
        switch self {
        case .ready: return "待命"
        case .authenticating: return "正在认证"
        case .authenticated: return "已认证"
        case .failed: return "认证失败"
        case .disconnected: return "已断开"
        case .unavailable: return "服务不可用"
        }
    }
    public var colorName: String {
        switch self {
        case .ready: return "blue"
        case .authenticating: return "orange"
        case .authenticated: return "green"
        case .failed: return "red"
        case .disconnected, .unavailable: return "gray"
        }
    }
}
