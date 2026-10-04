//! `history.visit.record` and `history.visit.title` are accepted only from
//! the app that hosts the browser (a local `frontend` connection).

use std::sync::Arc;

use serde_json::{Value, json};

use super::tests::captured_writer;
use super::{
    ClientTransport, ConnectionSurfaceScheduler, disconnect_client, handle_connection_message,
};
use crate::{Mux, SurfaceOptions};

fn send(mux: &Arc<Mux>, kind: Option<&str>, operation: &str, params: Value, key: &str) -> Value {
    let (writer, outbound) = captured_writer();
    let client = mux.control_clients.register(ClientTransport::Unix, writer.clone());
    if let Some(kind) = kind {
        mux.control_clients
            .set_info(client, Some("cmux-next".into()), Some(kind.into()), None)
            .unwrap();
    }
    let mut params = params;
    params["machine"] = json!("current");
    params["session"] = json!("current");
    let request = json!({
        "protocol": "cmux.protocol/2", "type": "request", "id": "gate",
        "operation": operation, "params": params, "idempotency_key": key,
    })
    .to_string();
    let scheduler =
        Arc::new(ConnectionSurfaceScheduler::new(mux.surface_operation_admission.clone()));
    assert!(handle_connection_message(mux, client, &request, &writer, &scheduler));
    let response = outbound.try_pop().expect("one resource response");
    disconnect_client(mux, client, false);
    serde_json::from_str(&response).unwrap()
}

fn visit() -> Value {
    json!({"profile": "default", "url": "https://example.com/", "at_ms": "1000"})
}

#[test]
fn visits_from_a_connection_that_is_not_the_hosting_app_are_refused() {
    let mux = Mux::new_for_test("history-gate", SurfaceOptions::default());
    for kind in [None, Some("cli"), Some("tui")] {
        let refused = send(&mux, kind, "history.visit.record", visit(), "v-refused");
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["error"]["details"]["reason"], "hosting_app_required", "{refused}");
        let title = json!({"profile": "default", "url": "https://example.com/", "title": "T"});
        let refused = send(&mux, kind, "history.visit.title", title, "t-refused");
        assert_eq!(refused["error"]["details"]["reason"], "hosting_app_required", "{refused}");
    }
}

#[test]
fn the_hosting_app_records_a_visit_and_sets_its_title() {
    let mux = Mux::new_for_test("history-gate-app", SurfaceOptions::default());
    let recorded = send(&mux, Some("frontend"), "history.visit.record", visit(), "v-1");
    assert_eq!(recorded["ok"], true, "{recorded}");
    let title = json!({"profile": "default", "url": "https://example.com/", "title": "Example"});
    let titled = send(&mux, Some("frontend"), "history.visit.title", title.clone(), "t-1");
    assert_eq!(titled["result"]["value"]["updated"], 1, "{titled}");
    let replayed = send(&mux, Some("frontend"), "history.visit.title", title, "t-1");
    assert_eq!(replayed["result"]["replayed"], true, "{replayed}");
    assert_eq!(replayed["result"]["value"]["updated"], 1, "{replayed}");
}
