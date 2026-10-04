@testable import CmuxNextApp
import CmuxNextDaemon
import CmuxNextHistory
import Foundation
import Testing

/// The daemon history module's wire (react-pages.md H3): entries map to the app's restore payloads,
/// locations stay the app's, and `history-changed` reaches the side-event bus.
struct DaemonHistoryEntryTests {
    static func decode(_ json: String) throws -> DaemonHistoryEntry {
        try JSONDecoder().decode(DaemonHistoryEntry.self, from: Data(json.utf8))
    }

    @Test func pageAgentCommandAndClosedEntriesMapToRestorePayloads() throws {
        let page = try Self.decode(#"{"id":"page:default:7","kind":"page","at_ms":1700000000000,"title":"Docs","url":"https://docs.rs/","profile":"work","available":true}"#)
        #expect(page.historyEntry()?.payload == .page(url: "https://docs.rs/", profile: "work"))
        #expect(page.historyEntry()?.time == Date(timeIntervalSince1970: 1_700_000_000))

        let agent = try Self.decode(#"{"id":"agent:local/claude/s1","kind":"agent","at_ms":1,"title":"Claude Code in repo","machine":"local","provider":"claude","session_id":"s1","cwd":"/r","running":true,"available":true}"#)
        guard case .agent(let session)? = agent.historyEntry()?.payload else { Issue.record("agent payload"); return }
        #expect(session.provider == "claude" && session.sessionID == "s1" && session.cwd == "/r" && session.endedAt == nil)

        let command = try Self.decode(#"{"id":"command:local/term_1/5","kind":"command","at_ms":1,"title":"ls","command":"ls -la","cwd":"/r","exit_code":0,"available":true}"#)
        guard case .command(let run)? = command.historyEntry()?.payload else { Issue.record("command payload"); return }
        #expect(run.command == "ls -la" && run.exitCode == 0)

        let closed = try Self.decode(#"{"id":"closed:daemon:closed_1","kind":"closed","at_ms":1,"title":"api","closed_kind":"workspace","available":true}"#)
        guard case .closed(let item)? = closed.historyEntry()?.payload else { Issue.record("closed payload"); return }
        #expect(item.id == "daemon:closed_1" && item.kind == .workspace)
        #expect(DaemonClosedHistory.daemonID(fromHistoryID: item.id) == "closed_1")
    }

    @Test func locationsStayTheAppsAndBrokenEntriesAreDropped() throws {
        #expect(try Self.decode(#"{"id":"location:local:tab_9:0","kind":"location","at_ms":1,"title":"zsh","available":true}"#).historyEntry() == nil)
        #expect(try Self.decode(#"{"id":"page:x:1","kind":"page","at_ms":1,"title":"no url","available":true}"#).historyEntry() == nil)
        #expect(try Self.decode(#"{"id":"agent:x","kind":"agent","at_ms":1,"title":"no session","available":false}"#).historyEntry() == nil)
    }

    @Test func visitSummariesDecode() throws {
        let rows = try JSONDecoder().decode([DaemonVisitSummary].self, from: Data(#"[{"url":"https://a.b/","title":"A","visit_count":3,"last_visit_ms":5000}]"#.utf8))
        #expect(rows == [DaemonVisitSummary(url: "https://a.b/", title: "A", visitCount: 3, lastVisitMS: 5000)])
    }

    @Test func historyChangedDecodesForTheSideEventBus() {
        let event = DaemonEvent.decode(name: "history-changed", line: Data(#"{"event":"history-changed","revision":9,"kinds":["page"]}"#.utf8))
        #expect(event == .historyChanged(revision: 9, kinds: ["page"]))
    }
}
