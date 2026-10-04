//! `history list|search|get|remove|remove-site|remove-url|clear-range|summaries`: the
//! session's history module (`history.*`, plans/cmux-next/react-pages.md
//! 2.3). The other `history` words (`back`, `show`, `clear`, `reopen`, ...)
//! are app actions and route to the app before this parser.

use cmux_tui_core::resource::ResourceOperation as Op;
use serde_json::{Map, Value};

use super::{
    CommandPlan, Flags, Selectors, UsageError, insert_bounded_u32, request, usage,
    validate_decimal, validate_one_of,
};

/// The `history` words the session host answers; every other word is an
/// app action.
pub(in crate::cli) const DAEMON_VERBS: &[&str] =
    &["list", "search", "get", "remove", "remove-site", "remove-url", "clear-range", "summaries"];

const KINDS: &[&str] = &["page", "location", "closed", "command", "agent"];
const RANGES: &[&str] = &["hour", "today", "week", "month", "all"];

pub(super) fn parse_history(words: &[&str], flags: &mut Flags) -> Result<CommandPlan, UsageError> {
    let selectors = Selectors::default();
    let mut params = Map::new();
    let operation = match words {
        ["list"] => {
            query(flags, &mut params)?;
            Op::HistoryEntriesList
        }
        ["search", text @ ..] if !text.is_empty() => {
            params.insert("text".into(), Value::String(text.join(" ")));
            query(flags, &mut params)?;
            Op::HistoryEntriesList
        }
        ["get", id] => {
            params.insert("id".into(), Value::String(bounded("history entry id", id)?));
            Op::HistoryEntriesGet
        }
        ["remove", ids @ ..] if !ids.is_empty() => {
            let ids = ids
                .iter()
                .map(|id| bounded("history entry id", id).map(Value::String))
                .collect::<Result<Vec<_>, _>>()?;
            params.insert("ids".into(), Value::Array(ids));
            Op::HistoryEntriesRemove
        }
        ["remove-site", host] => {
            params.insert("host".into(), Value::String(bounded("host", host)?));
            profile(flags, &mut params);
            Op::HistorySiteRemove
        }
        ["remove-url", url] => {
            params.insert("url".into(), Value::String(bounded("url", url)?));
            params.insert("profile".into(), Value::String(flags.required("profile")?));
            Op::HistoryVisitRemove
        }
        ["clear-range"] => {
            let since = flags.take("since-ms");
            match (flags.take("range"), &since) {
                (Some(range), _) => {
                    validate_one_of("--range", &range, RANGES)?;
                    params.insert("range".into(), Value::String(range));
                }
                (None, Some(_)) => {}
                (None, None) => return Err(UsageError::new("give --range or --since-ms")),
            }
            if let Some(since) = since {
                validate_decimal("--since-ms", &since)?;
                params.insert("since_ms".into(), Value::String(since));
            }
            kinds(flags, &mut params)?;
            profile(flags, &mut params);
            day_start(flags, &mut params)?;
            Op::HistoryClear
        }
        ["summaries"] => {
            params.insert("profile".into(), Value::String(flags.required("profile")?));
            if let Some(limit) = flags.take("limit") {
                insert_bounded_u32(&mut params, "limit", "--limit", limit, 1, 5000)?;
            }
            Op::HistoryVisitSummaries
        }
        _ => return usage("history action"),
    };
    request(operation, &selectors, flags, params)
}

/// `--kind`, `--range`, `--limit` and `--local-day-start-ms` of a list.
fn query(flags: &mut Flags, params: &mut Map<String, Value>) -> Result<(), UsageError> {
    kinds(flags, params)?;
    if let Some(range) = flags.take("range") {
        validate_one_of("--range", &range, RANGES)?;
        params.insert("range".into(), Value::String(range));
    }
    if let Some(limit) = flags.take("limit") {
        insert_bounded_u32(params, "limit", "--limit", limit, 1, 5000)?;
    }
    day_start(flags, params)
}

/// `--kind page,agent`: a comma-separated list of entry kinds.
fn kinds(flags: &mut Flags, params: &mut Map<String, Value>) -> Result<(), UsageError> {
    let Some(value) = flags.take("kind") else { return Ok(()) };
    let mut kinds = Vec::new();
    for kind in value.split(',').map(str::trim).filter(|kind| !kind.is_empty()) {
        validate_one_of("--kind", kind, KINDS)?;
        kinds.push(Value::String(kind.to_string()));
    }
    params.insert("kinds".into(), Value::Array(kinds));
    Ok(())
}

fn profile(flags: &mut Flags, params: &mut Map<String, Value>) {
    if let Some(profile) = flags.take("profile") {
        params.insert("profile".into(), Value::String(profile));
    }
}

fn day_start(flags: &mut Flags, params: &mut Map<String, Value>) -> Result<(), UsageError> {
    if let Some(value) = flags.take("local-day-start-ms") {
        validate_decimal("--local-day-start-ms", &value)?;
        params.insert("local_day_start_ms".into(), Value::String(value));
    }
    Ok(())
}

fn bounded(what: &str, value: &str) -> Result<String, UsageError> {
    if value.is_empty() || value.len() > 1024 {
        return Err(UsageError::new(format!("{what} must contain 1 to 1024 UTF-8 bytes")));
    }
    Ok(value.to_string())
}
