//! The session's history owner state: page visit logs, the journal folds and
//! the history revision. One mutex guards it; no call takes the registry
//! lock while holding it except through `Mux` reads that never call back
//! into history.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cmux_history::{
    AgentSessionFold, EntryContext, HiddenHistory, HistoryEntry, HistoryError, HistoryKind,
    TerminalCommandFold,
};
use serde_json::Value;

use super::pages::{DIRECTORY, Pages};
use crate::Mux;

/// The machine name the folds qualify ids with. It matches the Swift app's
/// `MachineRegistry.localID`, so hides it stored (`local/<provider>/<id>`)
/// keep applying; a client that merges several daemons qualifies ids by
/// session itself.
pub(super) const LOCAL_MACHINE: &str = "local";

#[derive(Default)]
pub(crate) struct HistoryHost {
    state: Mutex<Option<HistoryState>>,
    feed_claimed: AtomicBool,
}

pub(super) struct HistoryState {
    pub(super) pages: Pages,
    agents: AgentSessionFold,
    commands: TerminalCommandFold,
    /// The last journal sequence scanned.
    cursor: u64,
    revision: u64,
}

impl HistoryState {
    fn new(directory: Option<PathBuf>) -> Self {
        Self {
            pages: Pages::new(directory),
            agents: AgentSessionFold::new(LOCAL_MACHINE),
            commands: TerminalCommandFold::new(LOCAL_MACHINE),
            cursor: 0,
            revision: 1,
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// Agent and command entries the hides leave visible.
    pub(super) fn journal_entries(
        &self,
        kinds: impl Fn(HistoryKind) -> bool,
        hidden: &HiddenHistory,
    ) -> Vec<HistoryEntry> {
        let context = EntryContext { local_machine: LOCAL_MACHINE, available: true };
        let mut entries = Vec::new();
        if kinds(HistoryKind::Agent) {
            entries.extend(self.agents.entries(&context, hidden));
        }
        if kinds(HistoryKind::Command) {
            entries.extend(self.commands.entries(&context, hidden));
        }
        entries
    }
}

impl HistoryHost {
    /// Runs `body` on the state, opening it on first use under the session
    /// state directory (`None`: page logs in memory).
    pub(super) fn with<T>(&self, mux: &Mux, body: impl FnOnce(&mut HistoryState) -> T) -> T {
        let mut guard = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
        let state = guard.get_or_insert_with(|| {
            HistoryState::new(mux.session_state_directory().map(|dir| dir.join(DIRECTORY)))
        });
        body(state)
    }

    pub(super) fn cursor(&self, mux: &Mux) -> u64 {
        self.with(mux, |state| state.cursor)
    }

    /// Folds records scanned after `from` through `scanned`. A concurrent
    /// fold that already moved the cursor wins; the folds also ignore
    /// sequences they have seen. Returns the kinds that changed.
    pub(super) fn fold(
        &self,
        mux: &Mux,
        from: u64,
        scanned: u64,
        records: &[(HistoryKind, Value)],
    ) -> Vec<HistoryKind> {
        self.with(mux, |state| {
            if state.cursor != from {
                return Vec::new();
            }
            let mut changed = Vec::new();
            let agents: Vec<Value> = pick(records, HistoryKind::Agent);
            let commands: Vec<Value> = pick(records, HistoryKind::Command);
            if !agents.is_empty() {
                state.agents.apply(&agents);
                changed.push(HistoryKind::Agent);
            }
            if !commands.is_empty() {
                state.commands.apply(&commands);
                changed.push(HistoryKind::Command);
            }
            state.cursor = scanned;
            changed
        })
    }

    /// Bumps the history revision and emits `history-changed`.
    pub(super) fn changed(&self, mux: &Mux, kinds: &[HistoryKind]) {
        let revision = self.with(mux, |state| {
            state.revision += 1;
            state.revision
        });
        mux.emit(crate::MuxEvent::HistoryChanged {
            revision,
            kinds: kinds.iter().map(|kind| kind.as_str().to_owned()).collect(),
        });
    }

    pub(super) fn claim_feed(&self) -> bool {
        !self.feed_claimed.swap(true, Ordering::AcqRel)
    }

    pub(super) fn release_feed(&self) {
        self.feed_claimed.store(false, Ordering::Release);
    }
}

fn pick(records: &[(HistoryKind, Value)], kind: HistoryKind) -> Vec<Value> {
    records.iter().filter(|(of, _)| *of == kind).map(|(_, value)| value.clone()).collect()
}

/// A visit-log failure as `operation.failed` with reason `store_failed`.
pub(super) fn store_failed(
    operation: &str,
    error: &HistoryError,
) -> crate::resource::ResourceError {
    crate::resource::ResourceError::operation_failed(
        operation,
        "store_failed",
        serde_json::json!({ "message": error.to_string() }),
    )
}
