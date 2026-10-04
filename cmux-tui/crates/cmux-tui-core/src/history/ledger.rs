//! History mutations in the session's `resource_mutations` ledger: a key
//! replays its first result, the same key with other arguments is
//! `idempotency.conflict`, and every committed mutation advances the session
//! resource revision (the pattern of `git_ops/checkpoint/ledger.rs`).

use serde_json::{Value, json};

use crate::Mux;
use crate::resource::ResourceError;
use crate::resource_router::{mutation_result, resource_operation_error};
use crate::workspace_registry::{ResourcePatch, WorkspaceMutation};

const ORIGIN: &str = "resource-api";

/// The fingerprint a key binds: the operation and its fields.
pub(super) fn fingerprint(operation: &str, fields: &serde_json::Map<String, Value>) -> Value {
    json!({ "operation": operation, "fields": fields })
}

/// The first reply of `key`, when it committed with this operation and
/// fingerprint; `idempotency.conflict` for other arguments.
pub(super) fn prior(
    mux: &Mux,
    key: &str,
    operation: &'static str,
    fingerprint: &Value,
) -> Result<Option<Value>, ResourceError> {
    let mutation = mutation(key, operation)?;
    let replay = mux
        .workspace_registry
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .replay_resource_patch(&mutation, operation, fingerprint)
        .map_err(|error| registry_error(operation, error))?;
    match replay {
        Some(replay) => mutation_result(mux, replay.result, replay.revision, true).map(Some),
        None => Ok(None),
    }
}

/// Commits `value` as the mutation's result at a new resource revision.
pub(super) fn commit(
    mux: &Mux,
    key: &str,
    operation: &'static str,
    fingerprint: &Value,
    value: &Value,
) -> Result<Value, ResourceError> {
    let mutation = mutation(key, operation)?;
    let mut registry = mux.workspace_registry.lock().unwrap_or_else(|poison| poison.into_inner());
    let commit = registry
        .commit_resource_patch(
            &mutation,
            operation,
            fingerprint,
            None,
            None,
            &ResourcePatch { changes: Vec::new() },
            value,
            &json!([]),
        )
        .map_err(|error| registry_error(operation, error))?;
    if !commit.replayed {
        mux.state.lock().unwrap_or_else(|poison| poison.into_inner()).resource_revision =
            commit.revision;
    }
    drop(registry);
    if !commit.replayed {
        mux.publish_resource_event();
    }
    mutation_result(mux, commit.result, commit.revision, commit.replayed)
}

fn mutation(key: &str, operation: &'static str) -> Result<WorkspaceMutation, ResourceError> {
    WorkspaceMutation::new(key, ORIGIN).map_err(|error| registry_error(operation, error))
}

fn registry_error(operation: &'static str, error: anyhow::Error) -> ResourceError {
    let mapped = resource_operation_error(error);
    if mapped.code == "idempotency.conflict" {
        return mapped;
    }
    ResourceError::operation_failed(
        operation,
        "store_failed",
        json!({ "message": format!("the session's mutation ledger failed: {}", mapped.message) }),
    )
}
