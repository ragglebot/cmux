import AppKit
import CmuxNextDaemon
import Observation

/// App screens on the app side (plans/cmux-next/app-screens.md 3). The
/// store owns each app's workspace (`workspace.ensure_app`); this side
/// renders the `app` tab with the app's own page, the one view type an app
/// screen, an app column and an "Open as Tab" tab all show.
extension AppsService {
    /// The page view of an `app` tab, mounted once per tab through the app's
    /// page provider (the same mount a page tab uses). Nil when the app is
    /// not installed and visible or has no page.
    func tabView(for tab: TabModel) -> NSView? {
        guard tab.kind == .app, let appID = tab.snapshot.app else { return nil }
        if let entry = appTabs[tab.id], entry.app == appID { return entry.view }
        guard let app = registry.app(appID), AppPanePage.opens(app) else { return nil }
        if appTabs[tab.id] != nil { releaseTabView(tab.id) }
        let view = pageProvider(for: appID).makeView(for: tab.id, in: nil)
        appTabs[tab.id] = (appID, view)
        return view
    }

    func existingTabView(_ key: String) -> NSView? { appTabs[key]?.view }

    /// The tab closed or its surface was dropped: unmount its page.
    func releaseTabView(_ key: String) {
        guard let entry = appTabs.removeValue(forKey: key) else { return }
        pageProvider(for: entry.app).tabClosed(key)
    }

    /// Ensures `appID`'s app workspace on the local store
    /// (`workspace.ensure_app {app, kind}`, idempotent per app) and returns
    /// its workspace resource id. `cmux.apps.open {as: "screen"}` calls
    /// this, then selects the workspace (`showAppWorkspace`).
    func ensureAppWorkspace(_ appID: String, kind: AppWorkspaceClient.Kind) async throws -> ResourceID {
        let local = services.machines.local
        guard local.supports(DaemonCapabilities.shared.appScreens), let connection = local.connection else {
            throw AppsServiceError.noAppScreens
        }
        guard let app = registry.app(appID), app.isActive else { throw AppsServiceError.unknownApp }
        return try await AppWorkspaceClient(connection).ensureApp(app.manifest.id, kind: kind).workspaceID
    }

    /// Shows the app workspace `id` in the active window once the local
    /// tree reports it (a just-created workspace arrives after its event).
    /// Waits until the tree has it or the caller's task is cancelled.
    func showAppWorkspace(_ id: ResourceID) async {
        let store = services.machines.local.store
        var found: WorkspaceModel?
        for await workspace in Observations({ store.workspaces.first { $0.resourceID == id } }) {
            if let workspace { found = workspace; break }
        }
        guard !Task.isCancelled, let workspace = found else { return }
        if let state = services.windows.active?.state {
            services.windows.show(workspaceID: workspace.id, in: state)
        } else {
            services.windows.reveal(workspaceID: workspace.id)
        }
    }
}
