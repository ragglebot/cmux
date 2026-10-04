//! History module tests: the ops end to end through the resource router on
//! a test session, the journal folds, and property tests of remove, clear
//! and hides (idempotent replay by key; clear never touches closed or
//! location entries).

use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::{hidden_doc, sources};
use crate::resource_router::handle_resource_message;
use crate::workspace_registry::WorkspaceMutation;
use crate::{Mux, MuxEvent, SurfaceOptions};

fn mux() -> Arc<Mux> {
    Mux::new_for_test("history", SurfaceOptions::default())
}

fn call(mux: &Arc<Mux>, operation: &str, params: Value, key: Option<&str>) -> Value {
    let mut params = params;
    params["machine"] = json!("current");
    params["session"] = json!("current");
    let mut message = json!({
        "protocol": "cmux.protocol/2",
        "type": "request",
        "id": format!("test-{operation}"),
        "operation": operation,
        "params": params,
    });
    if let Some(key) = key {
        message["idempotency_key"] = json!(key);
    }
    handle_resource_message(mux, &message.to_string()).unwrap()
}

fn ok(envelope: Value) -> Value {
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["result"].clone()
}

fn record(mux: &Arc<Mux>, key: &str, url: &str, at_ms: i64) -> String {
    let params = json!({
        "profile": "default", "url": url, "title": "T", "tab": "local/tab_1", "at_ms": at_ms.to_string(),
    });
    let result = ok(call(mux, "history.visit.record", params, Some(key)));
    result["value"]["id"].as_str().unwrap().to_string()
}

fn ids(mux: &Arc<Mux>, params: Value) -> Vec<String> {
    let result = ok(call(mux, "history.entries.list", params, None));
    result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap().to_string())
        .collect()
}

fn now() -> i64 {
    super::now_ms()
}

/// A trail document as the Swift app stores it (dates are seconds since
/// 2001-01-01).
fn put_trail(mux: &Arc<Mux>) {
    let entered = (now() as f64 / 1000.0) - 978_307_200.0 - 10.0;
    let trail = json!({
        "cursor": 0,
        "entries": [{
            "enteredAt": entered,
            "location": {
                "key": {"machine": "local", "tab": "tab_9"},
                "window": "w", "workspace": "ws", "pane": "p", "content": "terminal",
                "title": "zsh", "workspaceTitle": "Work", "cwd": "/repo", "isIncognito": false,
            },
        }],
    });
    let mutation = WorkspaceMutation::new("trail-1", "test").unwrap();
    mux.put_frontend_projection(
        &mutation,
        "cmux-next",
        "personal",
        "history.trail",
        1,
        None,
        &trail,
    )
    .unwrap();
}

fn history_changed(events: &crate::event_bus::MuxEventReceiver) -> Vec<Vec<String>> {
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let MuxEvent::HistoryChanged { kinds, .. } = event {
            seen.push(kinds);
        }
    }
    seen
}

#[test]
fn a_visit_is_recorded_listed_removed_and_announced() {
    let mux = mux();
    let events = mux.subscribe();
    let id = record(&mux, "visit-1", "https://example.com/a", now() - 1_000);
    assert!(id.starts_with("page:default:"));
    assert_eq!(ids(&mux, json!({"kinds": ["page"]})), std::slice::from_ref(&id));
    let entry = ok(call(&mux, "history.entries.get", json!({"id": id}), None));
    assert_eq!(entry["url"], "https://example.com/a");
    assert!(entry["at_ms"].is_string());
    let removed = ok(call(&mux, "history.entries.remove", json!({"ids": [id]}), Some("rm-1")));
    assert_eq!(removed["value"]["removed"], 1);
    assert!(ids(&mux, json!({"kinds": ["page"]})).is_empty());
    let gone = call(&mux, "history.entries.get", json!({"id": id}), None);
    assert_eq!(gone["error"]["code"], "resource.not_found", "{gone}");
    assert_eq!(history_changed(&events), [vec!["page".to_string()], vec!["page".to_string()]]);
}

#[test]
fn a_replayed_key_returns_its_first_reply_and_other_arguments_conflict() {
    let mux = mux();
    let at_ms = now() - 1_000;
    let first = record(&mux, "visit-1", "https://example.com/a", at_ms);
    let again = record(&mux, "visit-1", "https://example.com/a", at_ms);
    assert_eq!(first, again);
    assert_eq!(ids(&mux, json!({"kinds": ["page"]})).len(), 1);
    let conflict = call(
        &mux,
        "history.visit.record",
        json!({"profile": "default", "url": "https://other/", "tab": "t", "at_ms": "1"}),
        Some("visit-1"),
    );
    assert_eq!(conflict["error"]["code"], "idempotency.conflict", "{conflict}");
}

#[test]
fn removing_a_location_is_refused_and_changes_nothing() {
    let mux = mux();
    put_trail(&mux);
    let page = record(&mux, "visit-1", "https://example.com/a", now() - 1_000);
    let locations = ids(&mux, json!({"kinds": ["location"]}));
    assert_eq!(locations, ["location:local:tab_9:0"]);
    let refused =
        call(&mux, "history.entries.remove", json!({"ids": [page, locations[0]]}), Some("rm-1"));
    assert_eq!(refused["error"]["code"], "operation.failed", "{refused}");
    assert_eq!(refused["error"]["details"]["reason"], super::writes::LOCATION_CLIENT_OWNED);
    assert_eq!(ids(&mux, json!({"kinds": ["page"]})), [page]);
}

#[test]
fn removing_agent_and_command_ids_hides_them_in_the_projection() {
    let mux = mux();
    let removed = ok(call(
        &mux,
        "history.entries.remove",
        json!({"ids": ["agent:local/claude/s1", "command:local/term_1/5", "closed:daemon:closed_1"]}),
        Some("rm-hide"),
    ));
    assert_eq!(removed["value"]["removed"], 2);
    let (hidden, revision) = hidden_doc::load(&mux).unwrap();
    assert_eq!(hidden.entries(), ["local/claude/s1", "local/term_1/5"]);
    assert!(revision > 0);
}

#[test]
fn clear_deletes_visits_hides_journal_kinds_and_keeps_locations() {
    let mux = mux();
    put_trail(&mux);
    record(&mux, "visit-1", "https://example.com/a", now() - 1_000);
    let cleared = ok(call(&mux, "history.clear", json!({"range": "all"}), Some("clear-1")));
    assert_eq!(cleared["value"]["removed"], 1);
    assert!(ids(&mux, json!({"kinds": ["page"]})).is_empty());
    assert_eq!(ids(&mux, json!({"kinds": ["location"]})).len(), 1);
    let (hidden, _) = hidden_doc::load(&mux).unwrap();
    let kinds: Vec<_> = hidden.ranges().iter().map(|range| range.kind.clone()).collect();
    assert_eq!(kinds, [Some("agent".to_string()), Some("command".to_string())]);
}

#[test]
fn visit_remove_deletes_every_visit_of_a_url_and_replays() {
    let mux = mux();
    let events = mux.subscribe();
    record(&mux, "v-1", "https://example.com/a", now() - 3_000);
    record(&mux, "v-2", "https://example.com/a", now() - 2_000);
    let keep = record(&mux, "v-3", "https://example.com/b", now() - 1_000);
    let params = json!({"profile": "default", "url": "https://example.com/a"});
    let removed = ok(call(&mux, "history.visit.remove", params.clone(), Some("rm-url")));
    assert_eq!(removed["value"]["removed"], 2);
    let replayed = ok(call(&mux, "history.visit.remove", params, Some("rm-url")));
    assert_eq!(
        (replayed["value"]["removed"].clone(), replayed["replayed"].clone()),
        (json!(2), json!(true))
    );
    assert_eq!(ids(&mux, json!({"kinds": ["page"]})), [keep]);
    assert_eq!(
        history_changed(&events).len(),
        4,
        "three records and one remove; no event for the replay"
    );
}

#[test]
fn clear_since_ms_replaces_the_range_start() {
    let mux = mux();
    let old = record(&mux, "v-old", "https://example.com/old", now() - 60_000);
    record(&mux, "v-new", "https://example.com/new", now() - 1_000);
    let since = (now() - 30_000).to_string();
    let cleared = ok(call(
        &mux,
        "history.clear",
        json!({"kinds": ["page"], "range": "all", "since_ms": since}),
        Some("clear-since"),
    ));
    assert_eq!(cleared["value"]["removed"], 1);
    assert_eq!(ids(&mux, json!({"kinds": ["page"]})), [old]);
    let neither = call(&mux, "history.clear", json!({"kinds": ["page"]}), Some("clear-none"));
    assert_eq!(neither["error"]["code"], "validation.invalid", "{neither}");
}

#[test]
fn the_folds_turn_journal_records_into_entries_minus_hides() {
    let mux = mux();
    let records = vec![
        (
            cmux_history::HistoryKind::Agent,
            json!({"sequence": 1, "kind": "agent.session.started", "occurred_at_ms": 1_000,
                   "payload": {"adapter": {"id": "codex"}, "normalized": {"agent_session_id": "a", "cwd": "/w"}}}),
        ),
        (
            cmux_history::HistoryKind::Command,
            json!({"sequence": 2, "kind": "shell.command.finished",
                   "subjects": [{"kind": "terminal", "id": "term_1"}],
                   "payload": {"command": "make", "cwd": "/w", "exit_code": 0, "started_at_ms": "2000"}}),
        ),
    ];
    let cursor = mux.history.cursor(&mux);
    let changed = mux.history.fold(&mux, cursor, cursor + 2, &records);
    assert_eq!(changed.len(), 2);
    let entries = mux
        .history
        .with(&mux, |state| state.journal_entries(|_| true, &cmux_history::HiddenHistory::new()));
    let mut ids: Vec<_> = entries.iter().map(|entry| entry.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, ["agent:local/codex/a", "command:local/term_1/2000"]);
}

#[test]
fn closed_items_and_trail_entries_map_to_entries() {
    let item = json!({"id": "closed_1", "kind": "tab", "name": null, "closed_at_ms": "5",
        "screens": [{"name": null, "tabs": [{"kind": "browser", "name": null, "url": "https://x/", "cwd": null}]}]});
    let entry = sources::closed_entry(&item).unwrap();
    assert_eq!(entry.id, "closed:daemon:closed_1");
    assert_eq!(entry.title, "https://x/");
    assert_eq!(entry.closed_kind, Some(cmux_history::ClosedKind::BrowserTab));
    let trail = json!({"cursor": 1, "entries": [
        {"enteredAt": 0.0, "location": {"key": {"machine": "m", "tab": "t"}, "title": "a", "isIncognito": false}},
        {"enteredAt": 1.0, "location": {"key": {"machine": "m", "tab": "u"}, "title": "b", "isIncognito": true}},
    ]});
    let entries = sources::location_entries(&trail);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].at_ms, 978_307_200_000);
    assert_eq!(entries[0].current, Some(false));
}

#[derive(Clone, Debug)]
enum Step {
    Record(u8),
    RemoveFirst,
    Clear,
    Replay,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0u8..4).prop_map(Step::Record),
        Just(Step::RemoveFirst),
        Just(Step::Clear),
        Just(Step::Replay),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// Any sequence of records, removes, clears and replays: a replayed key
    /// returns its first reply, and the location trail never changes.
    #[test]
    fn replays_are_stable_and_locations_survive(steps in prop::collection::vec(step(), 1..12)) {
        let mux = mux();
        put_trail(&mux);
        let mut replies: Vec<(String, &'static str, Value, Value)> = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let key = format!("key-{index}");
            let (operation, params) = match step {
                Step::Record(n) => ("history.visit.record", json!({
                    "profile": "default", "url": format!("https://h{n}.example/"), "tab": "t",
                    "at_ms": (now() - 5_000).to_string(),
                })),
                Step::RemoveFirst => {
                    let first = ids(&mux, json!({"kinds": ["page"]})).into_iter().next();
                    ("history.entries.remove", json!({"ids": [first.unwrap_or_else(|| "page:default:1".into())]}))
                }
                Step::Clear => ("history.clear", json!({"range": "hour"})),
                Step::Replay => match replies.first() {
                    Some((key, operation, params, reply)) => {
                        let again = ok(call(&mux, operation, params.clone(), Some(key)));
                        prop_assert_eq!(&again["value"], &reply["value"]);
                        prop_assert_eq!(&again["replayed"], &json!(true));
                        continue;
                    }
                    None => continue,
                },
            };
            let reply = ok(call(&mux, operation, params.clone(), Some(&key)));
            replies.push((key, operation, params, reply));
            prop_assert_eq!(ids(&mux, json!({"kinds": ["location"]})), vec!["location:local:tab_9:0".to_string()]);
        }
    }
}
