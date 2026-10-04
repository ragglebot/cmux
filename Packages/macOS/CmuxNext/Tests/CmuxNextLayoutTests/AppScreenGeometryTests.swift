import AppKit
import CmuxNextDesign
import Testing
@testable import CmuxNextLayout

/// App screens in the layout (plans/cmux-next/app-screens.md 3): an app
/// column draws without chrome; alone on its screen it fills the viewport,
/// and docked left it meets the window edge. Default style: gap 6, no pane
/// padding, 1000 x 600 viewport, scale 2.
@Suite struct AppScreenGeometryTests {
    let viewport = CGSize(width: 1000, height: 600)
    let style = LayoutStyle()

    private func geometry(_ layout: ScreenLayout) -> ScreenGeometry {
        ScreenGeometry.compute(layout, viewport: viewport, style: style, scale: 2)
    }

    @Test func anAppColumnAloneFillsTheScreenAndNothingScrolls() {
        let layout = ScreenLayout.columns([LayoutColumn(id: "app", width: 0.3, root: .leaf("p"), app: "app-store")])
        let g = geometry(layout)
        #expect(g.panes["p"] == CGRect(origin: .zero, size: viewport))
        #expect(g.columnEdges.isEmpty)
        #expect(g.gapZones.isEmpty)
        #expect(g.maxOffset == 0)
        #expect(layout.chromelessPanes == ["p"])
    }

    /// A screen of only the docked app column (an `appColumn` screen with no
    /// ordinary column) also fills the screen (the E2 exception).
    @Test func aLoneDockedAppColumnFillsTheScreen() {
        let layout = ScreenLayout.columns([
            LayoutColumn(id: "home", width: 0.3, root: .leaf("h"), sticky: StickyColumn(edge: .left, mode: .docked), app: "home"),
        ])
        #expect(geometry(layout).panes["h"] == CGRect(origin: .zero, size: viewport))
    }

    /// Home with no ordinary column yet: a tab dropped anywhere on the
    /// screen opens the first ordinary column right of the app column
    /// (never a drop into the locked app pane), and the left edge, which
    /// the app column holds, offers no dock.
    @Test func aLoneAppColumnTakesATabDropAsTheFirstColumnAfterIt() {
        let layout = ScreenLayout.columns([
            LayoutColumn(id: "home", width: 0.3, root: .leaf("h"), sticky: StickyColumn(edge: .left, mode: .docked), app: "home"),
        ])
        let g = geometry(layout)
        for point in [CGPoint(x: 500, y: 300), CGPoint(x: 100, y: 20), CGPoint(x: 900, y: 550)] {
            #expect(DropZoneGeometry.target(atView: point, offset: 0, screen: "s", geometry: g, style: style)
                    == .newColumn(screen: "s", after: "home"))
        }
        #expect(DropZoneGeometry.highlightRectInView(for: .newColumn(screen: "s", after: "home"), offset: 0, geometry: g, style: style)
                == CGRect(origin: .zero, size: viewport))
        #expect(DropZoneGeometry.dockTarget(atView: CGPoint(x: 2, y: 300), screen: "s", geometry: g, style: style) == nil)
        #expect(DropZoneGeometry.dockTarget(atView: CGPoint(x: 998, y: 300), screen: "s", geometry: g, style: style)
                == .newDock(screen: "s", edge: .right))
    }

    /// An `app` screen never grows a column: no new-column target there.
    @Test func anAppScreenOffersNoNewColumn() {
        let layout = ScreenLayout.columns([LayoutColumn(id: "app", width: 1, root: .leaf("p"), app: "app-store")])
        let g = geometry(layout)
        let target = DropZoneGeometry.target(atView: CGPoint(x: 500, y: 300), offset: 0, screen: "s", geometry: g, style: style)
        #expect(target != .newColumn(screen: "s", after: "app"))
        #expect(g.gapZones.isEmpty)
    }

    @Test func aDockedAppColumnMeetsTheLeftEdgeAndKeepsTheStrip() {
        let layout = ScreenLayout.columns([
            LayoutColumn(id: "home", width: 0.3, root: .leaf("h"), sticky: StickyColumn(edge: .left, mode: .docked), app: "home"),
            LayoutColumn(id: "c1", width: 0.5, root: .leaf("a")),
            LayoutColumn(id: "c2", width: 0.5, root: .leaf("b")),
        ])
        let g = geometry(layout)
        // An ordinary left dock sits at x 6, 292 wide; the app column takes the gap.
        #expect(g.panes["h"] == CGRect(x: 0, y: 0, width: 298, height: 600))
        #expect(g.fixedPanes == ["h"])
        #expect(g.stripMinX == 298)
        #expect(g.columnOrder == ["c1", "c2"])
        #expect(g.sticky.first?.cover.minX == 0)
        #expect(layout.chromelessPanes == ["h"])
    }

    @Test func ordinaryColumnsKeepTheirChrome() {
        let layout = ScreenLayout.columns([
            LayoutColumn(id: "c0", width: 0.3, root: .leaf("a"), sticky: StickyColumn(edge: .left, mode: .docked)),
            LayoutColumn(id: "c1", width: 0.5, root: .leaf("b")),
        ])
        #expect(layout.chromelessPanes.isEmpty)
        #expect(geometry(layout).panes["a"]?.minX == 6)
        #expect(ScreenLayout.splits(.leaf("x")).chromelessPanes.isEmpty)
    }

    /// Gaining or losing the app mark is a structural change (no animation
    /// between a chromed and a chromeless pane).
    @Test func theAppMarkIsStructural() {
        let plain = ScreenLayout.columns([LayoutColumn(id: "c", width: 1, root: .leaf("p"))])
        let app = ScreenLayout.columns([LayoutColumn(id: "c", width: 1, root: .leaf("p"), app: "home")])
        #expect(!plain.hasSameStructure(as: app))
        #expect(LayoutScreen(id: "s", name: "", layout: plain).kind == .workspace)
        #expect(LayoutScreenKind.appColumn("home").app == "home")
        #expect(LayoutScreenKind.workspace.app == nil)
    }
}

extension LayoutDesignMetricsTests {
    /// With pane padding, rounding and a border on, an app column pane still
    /// draws edge to edge with no ring, border or rounding, while the
    /// ordinary pane beside it keeps all three.
    @Test func appColumnPanesDrawWithoutChrome() async throws {
        let layout = ScreenLayout.columns([
            LayoutColumn(id: "home", width: 0.3, root: .leaf("h"), sticky: StickyColumn(edge: .left, mode: .docked), app: "home"),
            LayoutColumn(id: "c1", width: 0.7, root: .leaf("a")),
        ])
        let model = LayoutModel(screens: [LayoutScreen(id: "s", name: "", layout: layout, kind: .appColumn("home"))],
                                activeScreenID: "s", focusedPane: "h")
        let provider = StubProvider()
        let view = LayoutRootView(model: model, contentProvider: provider)
        view.frame = CGRect(origin: .zero, size: CGSize(width: 1000, height: 400))
        view.layoutSubtreeIfNeeded()
        try await withPaneChrome(PaneChromeOverrides(padding: 4, cornerRadius: 6, border: .subtle)) {
            try await waitUntil {
                guard let plain = view.context.hosts["a"] else { return false }
                return plain.contentRect.minX == 4
            }
            let home = try #require(view.context.hosts["h"])
            let plain = try #require(view.context.hosts["a"])
            #expect(home.contentRect == home.bounds)
            #expect(home.content.superview?.layer?.cornerRadius == 0)
            #expect(!home.chrome.showsRing)
            #expect(!home.chrome.showsBorder)
            #expect(plain.contentRect == plain.bounds.insetBy(dx: 4, dy: 4))
            #expect(plain.chrome.showsBorder)
        }
        withExtendedLifetime(provider) {}
    }
}
