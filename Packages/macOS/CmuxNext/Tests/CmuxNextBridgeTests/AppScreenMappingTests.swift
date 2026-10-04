import CmuxNextDaemon
import CmuxNextLayout
import Foundation
import Testing
@testable import CmuxNextBridge

/// App screens reach the layout (plans/cmux-next/app-screens.md 3) only from
/// a daemon that serves `app-screens-v1`; without it every screen maps as an
/// ordinary screen.
@MainActor
struct AppScreenMappingTests {
    private func workspace(_ screens: [ScreenSnapshot]) throws -> WorkspaceModel {
        let snapshot = WorkspaceSnapshot(id: 1, key: WorkspaceKey(rawValue: "0b6c4a52-6d3f-4c55-9d53-8f1f4e0f1a02"), name: "App",
                                         screens: screens)
        let store = DaemonStore()
        store.apply(snapshot: DaemonTree(workspaceRevision: 1, workspaces: [snapshot]))
        return try #require(store.workspaces.first)
    }

    private var appTab: TabSnapshot { TabSnapshot(surface: 5, kind: .app, title: "App Store", app: "app-store") }

    @Test func anAppScreenIsOneChromelessColumnOverTheScreen() throws {
        let screen = ScreenSnapshot(id: 4, layout: .leaf(3), panes: [PaneSnapshot(id: 3, tabs: [appTab])], kind: .app, app: "app-store")
        let mapped = try #require(LayoutMapping.shared.map(try workspace([screen]), appScreens: true).screens.first)
        #expect(mapped.kind == .app("app-store"))
        let columns = mapped.layout.columns
        #expect(columns.count == 1)
        #expect(columns.first?.id == mapped.implicitColumnID)
        #expect(columns.first?.app == "app-store")
        #expect(columns.first?.sticky == nil)
        #expect(mapped.layout.chromelessPanes == Set(mapped.layout.panes))
        #expect(mapped.layout.panes.count == 1)
    }

    @Test func anAppColumnScreenMarksOnlyTheAppColumn() throws {
        let screen = ScreenSnapshot(
            id: 4, layout: .leaf(3),
            columns: [
                ColumnSnapshot(id: 9, width: 0.3, layout: .leaf(3), sticky: StickySnapshot(edge: .left, mode: .docked), app: "home"),
                ColumnSnapshot(id: 8, width: 0.7, layout: .leaf(7)),
            ],
            panes: [PaneSnapshot(id: 3, tabs: [TabSnapshot(surface: 5, kind: .app, app: "home")]),
                    PaneSnapshot(id: 7, tabs: [TabSnapshot(surface: 6)])],
            kind: .appColumn, app: "home")
        let mapped = try #require(LayoutMapping.shared.map(try workspace([screen]), appScreens: true).screens.first)
        #expect(mapped.kind == .appColumn("home"))
        #expect(mapped.layout.columns.map(\.app) == ["home", nil])
        #expect(mapped.layout.columns.first?.sticky == StickyColumn(edge: .left, mode: .docked))
        #expect(mapped.layout.chromelessPanes.count == 1)
        // A drop after the app column (TabDragSession.columnAnchor) names it
        // to the daemon as `afterColumn`.
        let handles = LayoutMapping.shared.map(try workspace([screen]), appScreens: true).handles
        #expect(handles.columns[LayoutColumnID("column:9")] == DaemonColumnID(rawValue: 9))
    }

    /// The daemon sends an `appColumn` screen with no ordinary column
    /// without `columns`: it maps to the lone docked app column, so it
    /// draws chromeless over the screen and takes a tab drop as the first
    /// column after it.
    @Test func aLoneAppColumnScreenMapsToItsAppColumn() throws {
        let screen = ScreenSnapshot(id: 4, layout: .leaf(3), panes: [PaneSnapshot(id: 3, tabs: [TabSnapshot(surface: 5, kind: .app, app: "home")])],
                                    kind: .appColumn, app: "home")
        let mapped = try #require(LayoutMapping.shared.map(try workspace([screen]), appScreens: true).screens.first)
        #expect(mapped.kind == .appColumn("home"))
        let column = try #require(mapped.layout.columns.first)
        #expect(mapped.layout.columns.count == 1)
        #expect(column.id == mapped.implicitColumnID)
        #expect(column.app == "home")
        #expect(column.sticky == StickyColumn(edge: .left, mode: .docked))
        #expect(mapped.layout.chromelessPanes == Set(mapped.layout.panes))
        let g = ScreenGeometry.compute(mapped.layout, viewport: CGSize(width: 1000, height: 600), style: LayoutStyle(), scale: 2)
        #expect(DropZoneGeometry.target(atView: CGPoint(x: 500, y: 300), offset: 0, screen: mapped.id, geometry: g, style: LayoutStyle())
                == .newColumn(screen: mapped.id, after: column.id))
    }

    /// Without `app-screens-v1` the same tree is an ordinary screen with a
    /// pinned column: no kind, no app mark, no chromeless pane.
    @Test func withoutTheCapabilityEveryScreenIsOrdinary() throws {
        let app = ScreenSnapshot(id: 4, layout: .leaf(3), panes: [PaneSnapshot(id: 3, tabs: [appTab])], kind: .app, app: "app-store")
        let column = ScreenSnapshot(
            id: 14, layout: .leaf(13),
            columns: [ColumnSnapshot(id: 19, width: 0.3, layout: .leaf(13), sticky: StickySnapshot(edge: .left, mode: .docked), app: "home"),
                      ColumnSnapshot(id: 18, width: 0.7, layout: .leaf(17))],
            panes: [PaneSnapshot(id: 13, tabs: [TabSnapshot(surface: 15)]), PaneSnapshot(id: 17, tabs: [TabSnapshot(surface: 16)])],
            kind: .appColumn, app: "home")
        let screens = LayoutMapping.shared.map(try workspace([app, column])).screens
        #expect(screens.map(\.kind) == [.workspace, .workspace])
        #expect(screens[0].layout == .splits(.leaf(screens[0].layout.panes[0])))
        #expect(screens.allSatisfy { $0.layout.chromelessPanes.isEmpty })
    }

    @Test func anOrdinaryScreenStaysOrdinary() throws {
        let screen = ScreenSnapshot(id: 4, layout: .leaf(3), panes: [PaneSnapshot(id: 3, tabs: [TabSnapshot(surface: 5)])])
        let mapped = try #require(LayoutMapping.shared.map(try workspace([screen]), appScreens: true).screens.first)
        #expect(mapped.kind == .workspace)
        #expect(mapped.layout.chromelessPanes.isEmpty)
    }
}
