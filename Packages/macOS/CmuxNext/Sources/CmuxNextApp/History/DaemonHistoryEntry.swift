import CmuxNextHistory
import Foundation

/// One entry of the daemon's merged history (`history.entries.list` / `get`; serde snake_case of
/// cmux-history's `HistoryEntry`).
struct DaemonHistoryEntry: Decodable, Sendable, Equatable {
    var id: String
    var kind: HistoryEntry.Kind
    var atMS: Double
    var title: String
    var detail: String?
    var machine: String?
    var workspace: String?
    var available: Bool
    var current: Bool?
    var running: Bool?
    var url: String?
    var profile: String?
    var closedKind: String?
    var cwd: String?
    var command: String?
    var exitCode: Int?
    var sessionID: String?
    var provider: String?

    enum CodingKeys: String, CodingKey {
        case id, kind, title, detail, machine, workspace, available, current, running, url, profile, cwd, command, provider
        case atMS = "at_ms"
        case closedKind = "closed_kind"
        case exitCode = "exit_code"
        case sessionID = "session_id"
    }

    var time: Date { Date(timeIntervalSince1970: atMS / 1_000) }

    /// The app's entry for the restore paths (history.open, palette pages). Locations are nil: the
    /// app owns the trail and reads its own entries.
    func historyEntry() -> HistoryEntry? {
        let payload: HistoryEntry.Payload
        switch kind {
        case .location:
            return nil
        case .page:
            guard let url else { return nil }
            payload = .page(url: url, profile: profile ?? "default")
        case .closed:
            let itemKind: ClosedItem.Kind = switch closedKind {
            case "browser_tab": .browserTab
            case "screen": .screen
            case "workspace": .workspace
            default: .terminalTab
            }
            // The daemon's entry id is `closed:<closed history id>`; the app's closed item id is the rest.
            let itemID = id.hasPrefix("closed:") ? String(id.dropFirst("closed:".count)) : id
            payload = .closed(ClosedItem(id: itemID, kind: itemKind, title: title, machine: machine ?? "local",
                                         workspace: workspace, cwd: cwd, url: url))
        case .agent:
            guard let sessionID, let provider else { return nil }
            payload = .agent(AgentSession(machine: machine ?? "local", provider: provider, sessionID: sessionID, cwd: cwd,
                                          workspace: workspace, startedAt: time, lastActivityAt: time,
                                          endedAt: running == true ? nil : time))
        case .command:
            payload = .command(TerminalCommand(machine: machine ?? "local", terminal: "", command: command, cwd: cwd,
                                               exitCode: exitCode, startedAt: time, duration: nil))
        }
        return HistoryEntry(id: id, kind: kind, time: time, title: title, detail: detail, machineName: machine,
                            isAvailable: available, payload: payload)
    }
}

struct DaemonHistoryList: Decodable, Sendable {
    var entries: [DaemonHistoryEntry]
}

/// `history.visit.summaries` row: the omnibox seed of a browser profile.
struct DaemonVisitSummary: Decodable, Sendable, Equatable {
    var url: String
    var title: String?
    var visitCount: Int
    var lastVisitMS: Double

    enum CodingKeys: String, CodingKey {
        case url, title
        case visitCount = "visit_count"
        case lastVisitMS = "last_visit_ms"
    }
}
