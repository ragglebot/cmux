public import CmuxNextDaemon
public import CmuxNextLayout

/// Maps one daemon workspace (screens, split trees, scrolling columns) into
/// `LayoutScreen`s keyed by durable ids.
///
/// Rules (REWRITE.md Integration notes): a daemon `stack` becomes one leaf
/// showing its expanded pane; a zoomed pane fills its screen; unknown node
/// types drop out and their sibling takes the space.
public struct LayoutMapping {
    public static let shared = Self()
    public struct Result: Equatable, Sendable {
        public var screens: [LayoutScreen]
        public var handles: LayoutHandleMap
    }

    /// - Parameter appScreens: The daemon serves `app-screens-v1`. Without
    ///   it every screen maps as an ordinary screen, whatever it carries.
    public func map(_ workspace: WorkspaceModel, appScreens: Bool = false) -> Result {
        var handles = LayoutHandleMap()
        var screens: [LayoutScreen] = []
        for screen in workspace.screens {
            var paneIDs: [DaemonPaneID: LayoutPaneID] = [:]
            for pane in screen.panes {
                let id = LayoutPaneID(pane.id)
                paneIDs[pane.handle] = id
                handles.addPane(id, handle: pane.handle)
            }
            let screenID = LayoutScreenID(screen.id)
            handles.screens[screenID] = screen.handle
            guard let layout = layout(of: screen, paneIDs: paneIDs, handles: &handles, appScreens: appScreens) else { continue }
            var mapped = LayoutScreen(id: screenID, name: screen.name ?? "", layout: layout)
            if appScreens { Self.markApp(&mapped, kind: screen.kind, app: screen.app) }
            screens.append(mapped)
        }
        return Result(screens: screens, handles: handles)
    }

    /// An app screen's kind (app-screens.md 3). An `app` screen's content is
    /// one app column, its only column (the screen's implicit column when
    /// the daemon stores it as one split tree), so the layout draws it
    /// without chrome over the whole screen. An `appColumn` screen keeps the
    /// daemon's columns; its app column is marked from `columns[].app`.
    static func markApp(_ screen: inout LayoutScreen, kind: ScreenKind, app: String?) {
        guard let app else { return }
        switch kind {
        case .workspace:
            return
        case .app:
            screen.kind = .app(app)
            switch screen.layout {
            case let .splits(root):
                screen.layout = .columns([LayoutColumn(id: screen.implicitColumnID, width: 1, root: root, app: app)])
            case let .columns(columns):
                screen.layout = .columns(columns.map { column in
                    var column = column
                    column.app = app
                    return column
                })
            }
        case .appColumn:
            screen.kind = .appColumn(app)
        }
    }

    func layout(of screen: ScreenModel, paneIDs: [DaemonPaneID: LayoutPaneID],
                handles: inout LayoutHandleMap, appScreens: Bool = false) -> ScreenLayout? {
        if let zoomed = screen.zoomedPane, let id = paneIDs[zoomed] {
            return .splits(.leaf(id))
        }
        if !screen.columns.isEmpty {
            let columns = screen.columns.compactMap { column -> LayoutColumn? in
                guard let root = node(column.layout, paneIDs: paneIDs, handles: &handles) else { return nil }
                let id = LayoutHandleMap.columnID(column.id)
                handles.columns[id] = column.id
                let width = min(max(column.width, ColumnWidthPreset.widthRange.lowerBound), ColumnWidthPreset.widthRange.upperBound)
                return LayoutColumn(id: id, width: width, root: root, sticky: column.sticky.map(Self.sticky),
                                    app: appScreens && screen.kind == .appColumn ? column.app : nil)
            }
            return columns.isEmpty ? nil : .columns(columns)
        }
        return node(screen.layout, paneIDs: paneIDs, handles: &handles).map(ScreenLayout.splits)
    }

    /// Daemon `columns[].sticky` or `columns[].dock` as the layout's dock.
    public nonisolated static func sticky(_ snapshot: StickySnapshot) -> StickyColumn {
        let edge: StickyEdge = switch snapshot.edge {
        case .left: .left
        case .right: .right
        case .top: .top
        case .bottom: .bottom
        }
        return StickyColumn(edge: edge, mode: snapshot.mode == .overlay ? .overlay : .docked)
    }

    /// The layout's dock as the daemon's value. Top and bottom need a daemon
    /// that serves `edge-docks-v1`; `StickyColumnHandlers.apply` and the
    /// intent sender check that before sending.
    public nonisolated static func snapshot(_ sticky: StickyColumn) -> StickySnapshot {
        let edge: StickySnapshot.Edge = switch sticky.edge {
        case .left: .left
        case .right: .right
        case .top: .top
        case .bottom: .bottom
        }
        return StickySnapshot(edge: edge, mode: sticky.mode == .overlay ? .overlay : .docked)
    }

    /// Converts one daemon layout node. Nil when nothing in it can be shown.
    public func node(_ node: LayoutNode, paneIDs: [DaemonPaneID: LayoutPaneID],
                            handles: inout LayoutHandleMap) -> SplitNode? {
        switch node {
        case .leaf(let pane):
            return paneIDs[pane].map(SplitNode.leaf)
        case .stack(let panes, let expanded):
            let shown = paneIDs[expanded] ?? panes.lazy.compactMap { paneIDs[$0] }.first
            return shown.map(SplitNode.leaf)
        case .split(let splitHandle, let direction, let ratio, let a, let b):
            let first = self.node(a, paneIDs: paneIDs, handles: &handles)
            let second = self.node(b, paneIDs: paneIDs, handles: &handles)
            guard let first else { return second }
            guard let second else { return first }
            let id = LayoutHandleMap.splitID(splitHandle, firstPane: first.panes.first)
            if let splitHandle { handles.splits[id] = splitHandle }
            let axis: SplitAxis = direction == .right ? .horizontal : .vertical
            let clamped = min(max(ratio, SplitRatio.range.lowerBound), SplitRatio.range.upperBound)
            return .split(id, axis: axis, ratio: clamped, a: first, b: second)
        case .unknown:
            return nil
        }
    }
}
