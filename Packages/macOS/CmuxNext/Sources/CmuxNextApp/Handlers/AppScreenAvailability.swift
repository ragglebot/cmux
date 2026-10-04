import CmuxNextActions
import CmuxNextBridge
import CmuxNextLayout

/// Up-front availability on app screens (plans/cmux-next/app-screens.md 2):
/// the actions the daemon refuses on an `app` screen (`app-screen-fixed`)
/// or on an `appColumn` screen's app column (`app-column-locked`) show
/// disabled with the reason on those targets, so menus, the palette and the
/// daemon agree. Only a daemon serving `app-screens-v1` marks screens, so on
/// any other daemon nothing here applies. Closing the screen stays allowed;
/// the app column keeps its width presets.
enum AppScreenAvailability {
    /// Refused on an `app` screen: anything that adds, moves or closes a
    /// tab, pane or column, or docks one.
    static let appScreenFixed: [ActionID] = [
        "newTab", "newTab.sameKind", "newTab.page", "openBrowser", "openBrowser.webkit", "openBrowser.chromium",
        "duplicateTab", "closeTab", "closePane",
        "splitRight", "splitDown", "splitLeft", "splitUp", "splitBrowserRight", "splitBrowserDown", "newPaneAutoLayout", "newColumn",
        "tab.moveToNewSplit", "tab.moveToNewColumn", "tab.moveToNewStickyColumn", "tab.moveToWorkspace", "tab.moveToNewWindow",
        "palette.moveTabToNewWorkspace", "pane.moveToNewWorkspace",
        "moveSurfaceToPreviousPane", "moveSurfaceToNextPane", "moveSurfaceToPaneLeft", "moveSurfaceToPaneRight",
        "moveSurfaceToPaneUp", "moveSurfaceToPaneDown",
        "swapPaneLeft", "swapPaneRight", "swapPaneUp", "swapPaneDown",
        "column.dock", "column.dockLeft", "column.dockRight", "column.dockTop", "column.dockBottom", "column.undock", "column.float",
    ]

    /// Refused on the app column: the same, plus moving the column (it is
    /// fixed at index 0, sticky left).
    static let appColumnLocked: [ActionID] = appScreenFixed + ["column.moveLeft", "column.moveRight"]

    static func bind(into registry: ActionRegistry, context ctx: AppActionContext) {
        for id in appColumnLocked {
            ActionTargetReasons.add(id, in: registry) { invocation in
                guard let (screen, column) = ColumnAvailability.resolved(invocation, ctx) else { return nil }
                return reason(for: id, screen: screen, column: column)
            }
        }
    }

    /// The refusal for running `id` on `column` of `screen`, or nil.
    static func reason(for id: ActionID, screen: LayoutScreen, column: CmuxNextLayout.LayoutColumn) -> String? {
        switch screen.kind {
        case .workspace:
            return nil
        case .app:
            return appScreenFixed.contains(id) ? RefusalStrings.appScreenFixed : nil
        case .appColumn:
            return column.app != nil && appColumnLocked.contains(id) ? RefusalStrings.appColumnLocked : nil
        }
    }
}
