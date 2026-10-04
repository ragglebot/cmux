import Foundation

/// What a screen shows (`app-screens-v1`, plans/cmux-next/app-screens.md 1):
/// columns of panes (`workspace`, every screen of an older daemon), one app
/// filling the screen (`app`), or a locked app column on the left beside
/// ordinary columns (`appColumn`). An unknown kind from a newer daemon reads
/// as `workspace`, the shape older clients see.
public enum ScreenKind: String, Sendable, Hashable, Decodable {
    case workspace, app, appColumn

    public init(from decoder: any Decoder) throws {
        self = Self(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .workspace
    }
}
