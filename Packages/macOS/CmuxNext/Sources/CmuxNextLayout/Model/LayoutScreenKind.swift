/// What a screen shows (daemon `screens[].kind`, `app-screens-v1`).
public nonisolated enum LayoutScreenKind: Hashable, Sendable {
    /// Columns of panes with tabs and splits.
    case workspace
    /// One app fills the screen.
    case app(String)
    /// A locked app column on the left beside ordinary columns.
    case appColumn(String)

    /// The app of an app screen.
    public var app: String? {
        switch self {
        case .workspace: nil
        case let .app(id), let .appColumn(id): id
        }
    }
}
