import Foundation

public struct ReleaseUpdate: Decodable, Equatable {
    public let latest_version: String
    public let release_url: String
    public let summary: String
    public let available: Bool
    public let checking: Bool
    public let checked_at: UInt64
    public let error: String
    private enum CodingKeys: String, CodingKey { case latest_version, release_url, summary, available, checking, checked_at, error }
    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        latest_version = try values.decodeIfPresent(String.self, forKey: .latest_version) ?? ""
        release_url = try values.decodeIfPresent(String.self, forKey: .release_url) ?? ""
        summary = try values.decodeIfPresent(String.self, forKey: .summary) ?? ""
        available = try values.decodeIfPresent(Bool.self, forKey: .available) ?? false
        checking = try values.decodeIfPresent(Bool.self, forKey: .checking) ?? false
        checked_at = try values.decodeIfPresent(UInt64.self, forKey: .checked_at) ?? 0
        error = try values.decodeIfPresent(String.self, forKey: .error) ?? ""
    }
    public var url: URL? { Self.trustedURL(release_url) }
    public static func trustedURL(_ value: String) -> URL? {
        guard let parts = URLComponents(string: value), parts.scheme == "https",
              ["github.com", "gitee.com"].contains(parts.host?.lowercased() ?? ""),
              parts.user == nil, parts.password == nil, parts.port == nil || parts.port == 443,
              !value.contains("\\") else { return nil }
        return parts.url
    }
    public func shouldNotify(ignored: String?, notified: String?) -> Bool {
        available && !checking && url != nil && latest_version != ignored && latest_version != notified
    }
}
