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
    /// Refused on both targets: anything that adds, moves or closes a tab,
    /// pane or column, or docks one.
    static let locked: [ActionID] = [
        "newTab", "newTab.sameKind", "newTab.page", "openBrowser", "openBrowser.webkit", "openBrowser.chromium",
        "duplicateTab", "closeTab", "closePane",
        "splitRight", "splitDown", "splitLeft", "splitUp", "splitBrowserRight", "splitBrowserDown", "newPaneAutoLayout", "newColumn",
        "tab.moveToNewSplit", "tab.moveToNewColumn", "tab.moveToNewStickyColumn", "tab.moveToWorkspace", "tab.moveToNewWindow",
        "palette.moveTabToNewWorkspace", "pane.moveToNewWorkspace",
        "moveSurfaceToPreviousPane", "moveSurfaceToNextPane", "moveSurfaceToPaneLeft", "moveSurfaceToPaneRight",
        "moveSurfaceToPaneUp", "moveSurfaceToPaneDown",
        "swapPaneLeft", "swapPaneRight", "swapPaneUp", "swapPaneDown",
        "column.dock", "column.dockLeft", "column.dockRight", "column.dockTop", "column.dockBottom", "column.undock", "column.float",
        "column.moveLeft", "column.moveRight",
    ]

    /// Refused on an `app` screen: the locked actions and the width presets
    /// (the app fills the screen), so the app reason wins over the lone
    /// column's "Add a second column first".
    static let appScreenFixed: [ActionID] = locked + [
        "column.widthOneThird", "column.widthHalf", "column.widthTwoThirds", "column.widthFull",
        "column.cycleWidth", "column.cycleWidthBack",
    ]

    /// Refused on the app column (fixed at index 0, sticky left); its width
    /// presets stay available.
    static let appColumnLocked: [ActionID] = locked

    static func bind(into registry: ActionRegistry, context ctx: AppActionContext) {
        for id in appScreenFixed {
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
