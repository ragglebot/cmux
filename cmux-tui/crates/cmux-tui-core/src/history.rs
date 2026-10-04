//! The session's history module (plans/cmux-next/react-pages.md 2.2 and
//! 2.3, history.md section 2; R62, decided 2026-10-04).
//!
//! Owner role: this module owns page visits (one SQLite log per browser
//! profile under `<session state dir>/history/`), the hides of journal
//! history (the personal projection `history.hidden`, of which it is the
//! only writer), and the merged read model. The merged model also reads
//! facts other owners keep: agent sessions and finished commands folded from
//! the session journal, closed items from the closed-history store, and
//! locations from the app's `history.trail` projection (the app owns the
//! trail; this module never writes it).
//!
//! The ops are `history.*` in `spec/resource-operations-v2.json` (canonical
//! names `cmux.history.*`); every change emits the raw `history-changed`
//! event on the subscribe stream. The data layer is the `cmux-history`
//! crate.

mod feed;
mod hidden_doc;
mod host;
mod ledger;
mod pages;
mod reads;
mod sources;
#[cfg(test)]
mod tests;
mod writes;

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::resource::{ResourceError, ResourceOperation as Op};
use crate::resource_router::ParsedResourceRequest;
use crate::{Mux, ResourceTarget};

pub(crate) use feed::start;
pub(crate) use host::HistoryHost;

/// Advertised in identify: the session host serves the `history.*` ops and
/// the `history-changed` event.
pub(crate) const HISTORY_CAPABILITY: &str = "history-v1";

pub(crate) fn handles(operation: Op) -> bool {
    matches!(
        operation,
        Op::HistoryEntriesList
            | Op::HistoryEntriesGet
            | Op::HistoryEntriesRemove
            | Op::HistorySiteRemove
            | Op::HistoryClear
            | Op::HistoryVisitRecord
            | Op::HistoryVisitSummaries
            | Op::HistoryVisitTitle
            | Op::HistoryVisitRemove
    )
}

pub(crate) fn dispatch(
    mux: &Arc<Mux>,
    request: ParsedResourceRequest,
) -> Result<Value, ResourceError> {
    debug_assert!(handles(request.envelope.operation));
    mux.resolve_resource_path(ResourceTarget::Session, &request.selectors)?;
    start(mux);
    let fields = &request.fields;
    let key = request.envelope.idempotency_key.clone().unwrap_or_default();
    match request.envelope.operation {
        Op::HistoryEntriesList => reads::list(mux, fields),
        Op::HistoryEntriesGet => reads::get(mux, fields),
        Op::HistoryVisitSummaries => reads::summaries(mux, fields),
        Op::HistoryEntriesRemove => writes::remove(mux, &key, fields),
        Op::HistorySiteRemove => writes::remove_site(mux, &key, fields),
        Op::HistoryClear => writes::clear(mux, &key, fields),
        Op::HistoryVisitRecord => writes::record_visit(mux, &key, fields),
        Op::HistoryVisitTitle => writes::visit_title(mux, &key, fields),
        Op::HistoryVisitRemove => writes::visit_remove(mux, &key, fields),
        other => unreachable!("history does not handle {other:?}"),
    }
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// The local start of today: the caller's `local_day_start_ms` (the page
/// knows the user's time zone), else the daemon's own local midnight.
pub(super) fn day_start_ms(fields: &Map<String, Value>, now_ms: i64) -> Result<i64, ResourceError> {
    match fields.get("local_day_start_ms") {
        Some(value) => value.as_str().and_then(|text| text.parse::<i64>().ok()).ok_or_else(|| {
            ResourceError::validation_invalid(
                Some("local_day_start_ms"),
                "local_day_start_ms must be an unsigned decimal string",
            )
        }),
        None => Ok(local_midnight_ms(now_ms)),
    }
}

/// Local midnight before `now_ms` from the C library's time zone offset.
fn local_midnight_ms(now_ms: i64) -> i64 {
    const DAY_S: i64 = 86_400;
    let now_s = now_ms.div_euclid(1000);
    let offset = utc_offset_s(now_s);
    ((now_s + offset).div_euclid(DAY_S) * DAY_S - offset) * 1000
}

#[cfg(unix)]
// `time_t` and `c_long` are 64 bits on the supported unixes but 32 bits on
// some others, so the conversions stay written out.
#[allow(
    irrefutable_let_patterns,
    clippy::unnecessary_fallible_conversions,
    clippy::useless_conversion
)]
fn utc_offset_s(now_s: i64) -> i64 {
    let Ok(time) = libc::time_t::try_from(now_s) else { return 0 };
    // SAFETY: localtime_r writes only into the zeroed `tm` we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call; the result is checked.
    let result = unsafe { libc::localtime_r(&time, &mut tm) };
    if result.is_null() { 0 } else { i64::from(tm.tm_gmtoff) }
}

#[cfg(not(unix))]
fn utc_offset_s(_now_s: i64) -> i64 {
    0
}
