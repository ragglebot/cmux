//! `history.entries.list`, `history.entries.get` and
//! `history.visit.summaries`: the merged read model over every owner.

use std::path::Path;

use cmux_history::{HistoryEntry, HistoryKind, HistoryQuery, HistoryRange};
use serde_json::{Map, Value, json};

use super::host::store_failed;
use super::pages::parse_page_id;
use super::{feed, hidden_doc, sources};
use crate::Mux;
use crate::resource::ResourceError;
use crate::resource_router::resource_operation_error;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 5_000;
const DEFAULT_SUMMARIES: usize = 5_000;
const TRAIL_SUBJECT: &str = "history.trail";

/// The query a list request names.
pub(super) fn query(fields: &Map<String, Value>) -> Result<HistoryQuery, ResourceError> {
    let kinds = match fields.get("kinds") {
        Some(value) => serde_json::from_value::<Vec<HistoryKind>>(value.clone())
            .map_err(|error| ResourceError::validation_invalid(Some("kinds"), error.to_string()))?,
        None => Vec::new(),
    };
    let range = range(fields, "range")?.unwrap_or(HistoryRange::All);
    let limit = fields
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_LIMIT, |limit| usize::try_from(limit).unwrap_or(MAX_LIMIT))
        .min(MAX_LIMIT);
    let text = fields.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(HistoryQuery { kinds, text, range, limit: Some(limit) })
}

pub(super) fn range(
    fields: &Map<String, Value>,
    field: &str,
) -> Result<Option<HistoryRange>, ResourceError> {
    fields
        .get(field)
        .map(|value| {
            serde_json::from_value::<HistoryRange>(value.clone())
                .map_err(|error| ResourceError::validation_invalid(Some(field), error.to_string()))
        })
        .transpose()
}

pub(super) fn list(mux: &Mux, fields: &Map<String, Value>) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.entries.list";
    let query = query(fields)?;
    let now = super::now_ms();
    let day_start = super::day_start_ms(fields, now)?;
    let since = query.range.start(now, day_start);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let (entries, revision) =
        gather(mux, OPERATION, |kind| query.wants(kind), &query.text, since, limit)?;
    let matched = query.apply(&entries, now, day_start, |path| Path::new(path).is_dir());
    Ok(json!({
        "entries": matched.iter().map(wire).collect::<Vec<_>>(),
        "revision": revision.to_string(),
    }))
}

pub(super) fn get(mux: &Mux, fields: &Map<String, Value>) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.entries.get";
    let id = fields.get("id").and_then(Value::as_str).unwrap_or_default();
    let now = super::now_ms();
    let found = if let Some((profile, visit)) = parse_page_id(id) {
        mux.history
            .with(mux, |state| state.pages.entry(profile, visit, now))
            .map_err(|error| store_failed(OPERATION, &error))?
    } else {
        let kind = id.split(':').next().and_then(kind_named);
        match kind {
            Some(kind) => {
                let (entries, _) = gather(mux, OPERATION, |of| of == kind, "", None, MAX_LIMIT)?;
                entries.into_iter().find(|entry| entry.id == id)
            }
            None => None,
        }
    };
    match found {
        Some(entry) => Ok(wire(&entry)),
        None => Err(resource_operation_error(crate::state::commit::state_not_found("history", id))),
    }
}

pub(super) fn summaries(mux: &Mux, fields: &Map<String, Value>) -> Result<Value, ResourceError> {
    const OPERATION: &str = "history.visit.summaries";
    let profile = fields.get("profile").and_then(Value::as_str).unwrap_or_default();
    let limit = fields
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_SUMMARIES, |limit| usize::try_from(limit).unwrap_or(DEFAULT_SUMMARIES));
    let now = super::now_ms();
    let summaries = mux
        .history
        .with(mux, |state| state.pages.summaries(profile, limit, now))
        .map_err(|error| store_failed(OPERATION, &error))?;
    Ok(Value::Array(
        summaries
            .iter()
            .map(|summary| {
                json!({
                    "url": summary.url,
                    "title": summary.title,
                    "visit_count": u32::try_from(summary.visit_count).unwrap_or(u32::MAX),
                    "last_visit_ms": summary.last_visit_ms.max(0).to_string(),
                })
            })
            .collect(),
    ))
}

/// Every entry of the selected kinds, before the query filters: page visits
/// (pre-filtered by `text`, at or after `since`, `limit` per profile), the
/// journal folds minus the hides, closed items and trail locations. Returns
/// the entries and the history revision.
fn gather(
    mux: &Mux,
    operation: &'static str,
    wants: impl Fn(HistoryKind) -> bool,
    text: &str,
    since: Option<i64>,
    limit: usize,
) -> Result<(Vec<HistoryEntry>, u64), ResourceError> {
    let failed = |error: anyhow::Error| {
        ResourceError::operation_failed(
            operation,
            "store_failed",
            json!({"message": format!("{error:#}")}),
        )
    };
    if wants(HistoryKind::Agent) || wants(HistoryKind::Command) {
        let changed = feed::catch_up(mux).map_err(failed)?;
        if !changed.is_empty() {
            mux.history.changed(mux, &changed);
        }
    }
    let (hidden, _) = hidden_doc::load(mux).map_err(failed)?;
    let now = super::now_ms();
    let (mut entries, revision) = mux.history.with(mux, |state| {
        let mut entries = state.journal_entries(&wants, &hidden);
        if wants(HistoryKind::Page) {
            // The visit log's SQL pre-filter compares folded tokens with raw
            // text (a Swift quirk kept in the crate: "resume" misses
            // "Résumé"), so a search reads the newest visits unfiltered and
            // the query folds both sides.
            let window = if text.is_empty() { limit } else { MAX_LIMIT };
            entries.extend(
                state
                    .pages
                    .entries("", since, window, now)
                    .map_err(|error| store_failed(operation, &error))?,
            );
        }
        Ok::<_, ResourceError>((entries, state.revision()))
    })?;
    if wants(HistoryKind::Closed) {
        let items = mux
            .read_registry_state(crate::state::closed_history_store::closed_items)
            .map_err(failed)?;
        entries.extend(items.iter().filter_map(sources::closed_entry));
    }
    if wants(HistoryKind::Location) {
        let trail = mux
            .get_frontend_projection(hidden_doc::FRONTEND, hidden_doc::SCOPE, TRAIL_SUBJECT)
            .map_err(failed)?;
        if let Some(trail) = trail {
            entries.extend(sources::location_entries(&trail.projection));
        }
    }
    Ok((entries, revision))
}

pub(super) fn kind_named(name: &str) -> Option<HistoryKind> {
    HistoryKind::ALL.into_iter().find(|kind| kind.as_str() == name)
}

/// The catalog shape of an entry: times as unsigned decimal strings.
pub(super) fn wire(entry: &HistoryEntry) -> Value {
    let mut value = serde_json::to_value(entry).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert("at_ms".into(), Value::String(entry.at_ms.max(0).to_string()));
    }
    value
}
