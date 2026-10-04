//! History mutations: `history.entries.remove`, `history.site.remove`,
//! `history.clear` and `history.visit.record`. Each one replays by its
//! idempotency key, changes its owners' stores, commits its result to the
//! session ledger, and emits `history-changed`.
//!
//! What each kind allows (history.md section 3): page visits are deleted;
//! agent and command entries live in the append-only journal and are hidden
//! (`history.hidden`); closed items age out in the closed-history store and
//! are left alone; locations are the app's view state and are refused.

use cmux_history::{HistoryKind, NewVisit, hidden_id};
use serde_json::{Map, Value, json};

use super::host::store_failed;
use super::pages::parse_page_id;
use super::reads::range;
use super::{hidden_doc, ledger};
use crate::Mux;
use crate::resource::ResourceError;

/// The reason `history.entries.remove` refuses a location id.
pub(super) const LOCATION_CLIENT_OWNED: &str = "history.location_client_owned";

/// Runs a mutation once per key: a replay returns the first reply; otherwise
/// `apply` makes the change and returns its result value and changed kinds.
pub(super) fn keyed(
    mux: &Mux,
    key: &str,
    operation: &'static str,
    fields: &Map<String, Value>,
    apply: impl FnOnce() -> Result<(Value, Vec<HistoryKind>), ResourceError>,
) -> Result<Value, ResourceError> {
    let fingerprint = ledger::fingerprint(operation, fields);
    if let Some(reply) = ledger::prior(mux, key, operation, &fingerprint)? {
        return Ok(reply);
    }
    let (value, changed) = apply()?;
    let reply = ledger::commit(mux, key, operation, &fingerprint, &value)?;
    if !changed.is_empty() {
        mux.history.changed(mux, &changed);
    }
    Ok(reply)
}

pub(super) fn remove(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.entries.remove";
    let ids: Vec<String> = fields
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(location) = ids.iter().find(|id| id.starts_with("location:")) {
        return Err(ResourceError::operation_failed(
            OPERATION,
            LOCATION_CLIENT_OWNED,
            json!({
                "id": location,
                "message": "locations belong to the app's trail; remove them in the app",
            }),
        ));
    }
    keyed(mux, key, OPERATION, fields, || {
        let now = super::now_ms();
        let mut removed = 0usize;
        let mut changed = Vec::new();
        for (profile, visit) in ids.iter().filter_map(|id| parse_page_id(id)) {
            removed += mux
                .history
                .with(mux, |state| state.pages.remove_visit(profile, visit, now))
                .map_err(|error| store_failed(OPERATION, &error))?;
        }
        if removed > 0 {
            changed.push(HistoryKind::Page);
        }
        let hides: Vec<(&str, HistoryKind)> = ids
            .iter()
            .filter_map(|id| {
                let kind = if id.starts_with("agent:") {
                    HistoryKind::Agent
                } else {
                    HistoryKind::Command
                };
                hidden_id(id).map(|hidden| (hidden, kind))
            })
            .collect();
        if !hides.is_empty() {
            hidden_doc::change(mux, key, |document| {
                for (id, _) in &hides {
                    document.hide_entry(id);
                }
            })
            .map_err(|error| failed(OPERATION, &error))?;
            removed += hides.len();
            for (_, kind) in &hides {
                if !changed.contains(kind) {
                    changed.push(*kind);
                }
            }
        }
        Ok((json!({ "removed": count(removed) }), changed))
    })
}

pub(super) fn remove_site(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.site.remove";
    let host = fields.get("host").and_then(Value::as_str).unwrap_or_default().to_owned();
    let profile = fields.get("profile").and_then(Value::as_str).map(str::to_owned);
    keyed(mux, key, OPERATION, fields, || {
        let now = super::now_ms();
        let removed = mux
            .history
            .with(mux, |state| state.pages.remove_host(&host, profile.as_deref(), now))
            .map_err(|error| store_failed(OPERATION, &error))?;
        let changed = if removed > 0 { vec![HistoryKind::Page] } else { Vec::new() };
        Ok((json!({ "removed": count(removed) }), changed))
    })
}

pub(super) fn clear(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.clear";
    let query = super::reads::query(fields)?;
    let profile = fields.get("profile").and_then(Value::as_str).map(str::to_owned);
    let now = super::now_ms();
    let since = match (fields.get("since_ms").and_then(Value::as_str), range(fields, "range")?) {
        (Some(since), _) => Some(since.parse::<i64>().map_err(|_| {
            ResourceError::validation_invalid(
                Some("since_ms"),
                "since_ms must be an unsigned decimal string",
            )
        })?),
        (None, Some(range)) => range.start(now, super::day_start_ms(fields, now)?),
        (None, None) => {
            return Err(ResourceError::validation_invalid(
                Some("range"),
                "history.clear needs range or since_ms",
            ));
        }
    };
    keyed(mux, key, OPERATION, fields, || {
        let mut changed = Vec::new();
        let mut removed = 0;
        if query.wants(HistoryKind::Page) {
            removed = mux
                .history
                .with(mux, |state| state.pages.clear(since, profile.as_deref(), now))
                .map_err(|error| store_failed(OPERATION, &error))?;
            changed.push(HistoryKind::Page);
        }
        let hidden: Vec<HistoryKind> = [HistoryKind::Agent, HistoryKind::Command]
            .into_iter()
            .filter(|kind| query.wants(*kind))
            .collect();
        if !hidden.is_empty() {
            hidden_doc::change(mux, key, |document| {
                for kind in &hidden {
                    document.hide_range(since, now, Some(kind.as_str()));
                }
            })
            .map_err(|error| failed(OPERATION, &error))?;
            changed.extend(hidden);
        }
        Ok((json!({ "removed": count(removed) }), changed))
    })
}

pub(super) fn record_visit(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.visit.record";
    let text = |field: &str| fields.get(field).and_then(Value::as_str).map(str::to_owned);
    let profile = text("profile").unwrap_or_default();
    let at_ms = text("at_ms").and_then(|value| value.parse::<i64>().ok()).unwrap_or_default();
    let visit = NewVisit {
        url: text("url").unwrap_or_default(),
        title: text("title"),
        tab: text("tab"),
        at_ms,
    };
    keyed(mux, key, OPERATION, fields, || {
        let now = super::now_ms();
        let id = mux
            .history
            .with(mux, |state| state.pages.record(&profile, &visit, now))
            .map_err(|error| store_failed(OPERATION, &error))?;
        Ok((json!({ "id": id }), vec![HistoryKind::Page]))
    })
}

pub(super) fn visit_title(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.visit.title";
    let text =
        |field: &str| fields.get(field).and_then(Value::as_str).unwrap_or_default().to_owned();
    let (profile, url, title) = (text("profile"), text("url"), text("title"));
    keyed(mux, key, OPERATION, fields, || {
        let now = super::now_ms();
        let changed = mux
            .history
            .with(mux, |state| state.pages.update_title(&profile, &url, &title, now))
            .map_err(|error| store_failed(OPERATION, &error))?;
        let kinds = if changed > 0 { vec![HistoryKind::Page] } else { Vec::new() };
        Ok((json!({ "updated": count(changed) }), kinds))
    })
}

pub(super) fn visit_remove(
    mux: &Mux,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.visit.remove";
    let text =
        |field: &str| fields.get(field).and_then(Value::as_str).unwrap_or_default().to_owned();
    let (profile, url) = (text("profile"), text("url"));
    keyed(mux, key, OPERATION, fields, || {
        let now = super::now_ms();
        let removed = mux
            .history
            .with(mux, |state| state.pages.remove_url(&profile, &url, now))
            .map_err(|error| store_failed(OPERATION, &error))?;
        let kinds = if removed > 0 { vec![HistoryKind::Page] } else { Vec::new() };
        Ok((json!({ "removed": count(removed) }), kinds))
    })
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn failed(operation: &str, error: &anyhow::Error) -> ResourceError {
    ResourceError::operation_failed(
        operation,
        "store_failed",
        json!({ "message": format!("{error:#}") }),
    )
}
