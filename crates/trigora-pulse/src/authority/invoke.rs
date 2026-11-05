use std::collections::BTreeMap;

use tcc_state::{BranchOp, Continuation, PendingOp, WaitKind};

use super::identity::{canonical_keys, is_terminal, sha256};
use super::state::{CreatedLink, State, StoredIntent};
use super::types::{CreatedChild, InvokeIntent, InvokeReceipt, InvokeSpec, StartedChild};
use crate::error::{Error, Result};

pub(super) fn record(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    fence: u64,
    spec: &InvokeSpec,
) -> Result<InvokeReceipt> {
    if spec.invoke_id.is_empty()
        || spec.child_execution_id.is_empty()
        || spec.program_name.is_empty()
    {
        return Err(Error::new("invoke intent is invalid"));
    }
    let clock = state.clock;
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    if record.owner.as_deref() != Some(owner_token)
        || record.fence != fence
        || record.lease_until <= clock
    {
        return Err(Error::new("stale owner"));
    }
    let digest = sha256(spec.arguments.as_bytes());
    if let Some(existing) = record.intents.get(&spec.child_execution_id) {
        if existing.invoke_id == spec.invoke_id
            && existing.program_name == spec.program_name
            && existing.digest == digest
        {
            return Ok(InvokeReceipt {
                revision: record.revision,
                idempotent: true,
            });
        }
        return Err(Error::new("invoke conflict"));
    }
    record.intents.insert(
        spec.child_execution_id.clone(),
        StoredIntent {
            invoke_id: spec.invoke_id.clone(),
            child_execution_id: spec.child_execution_id.clone(),
            program_name: spec.program_name.clone(),
            arguments: spec.arguments.clone(),
            digest,
            committed: false,
        },
    );
    Ok(InvokeReceipt {
        revision: record.revision,
        idempotent: false,
    })
}

/// Create every child named by an uncommitted intent, or the one child an
/// offered spec names. The parent's lease is not consulted. An offered spec
/// that differs from the stored intent conflicts and writes nothing.
pub(super) fn ensure(
    state: &mut State,
    parent_id: &str,
    offered: Option<&InvokeSpec>,
) -> Result<InvokeReceipt> {
    let (revision, links) = {
        let parent = state
            .executions
            .get(parent_id)
            .ok_or_else(|| Error::new("unknown execution"))?;
        let selected: Vec<&StoredIntent> = if let Some(offered) = offered {
            let Some(intent) = parent.intents.get(&offered.child_execution_id) else {
                return Err(Error::new("invoke intent is missing"));
            };
            if offered.invoke_id != intent.invoke_id
                || offered.program_name != intent.program_name
                || offered.arguments != intent.arguments
            {
                return Err(Error::new("child conflict"));
            }
            vec![intent]
        } else {
            let pending: Vec<&StoredIntent> = parent
                .intents
                .values()
                .filter(|intent| !intent.committed)
                .collect();
            if pending.is_empty() {
                return Err(Error::new("invoke intent is missing"));
            }
            pending
        };
        let mut links = Vec::with_capacity(selected.len());
        for intent in selected {
            if intent.child_execution_id == parent_id {
                return Err(Error::new("child conflict"));
            }
            links.push((
                intent.child_execution_id.clone(),
                CreatedLink {
                    parent_execution_id: parent_id.to_string(),
                    invoke_id: intent.invoke_id.clone(),
                    program_name: intent.program_name.clone(),
                    arguments: intent.arguments.clone(),
                    digest: intent.digest.clone(),
                },
            ));
        }
        (parent.revision, links)
    };
    let mut idempotent = true;
    for (child_id, link) in links {
        if let Some(existing) = state.executions.get(&child_id) {
            let same = existing.created.as_ref().is_some_and(|created| {
                created.parent_execution_id == link.parent_execution_id
                    && created.invoke_id == link.invoke_id
                    && created.program_name == link.program_name
                    && created.arguments == link.arguments
                    && created.digest == link.digest
            });
            if !same {
                return Err(Error::new("child conflict"));
            }
            continue;
        }
        idempotent = false;
        let mut record = super::state::Record::new(String::new(), 0, 0);
        record.owner = None;
        record.created = Some(link);
        state.executions.insert(child_id, record);
    }
    Ok(InvokeReceipt {
        revision,
        idempotent,
    })
}

/// Take the child's lease and return the stored arguments. The continuation is
/// not written here. A committed continuation makes a later call resume instead
/// of initializing a second engine.
pub(super) fn start(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    lease_ms: u64,
    artifact_hash: &str,
    handler_keys: &[String],
) -> Result<StartedChild> {
    let clock = state.clock;
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    let Some(link) = record.created.clone() else {
        return Err(Error::new("unknown execution"));
    };
    let keys = canonical_keys(handler_keys);
    if !record.artifact_hash.is_empty()
        && (record.artifact_hash != artifact_hash || record.handler_keys != keys)
    {
        return Err(Error::new("artifact version mismatch"));
    }
    let initialized = record.continuation.is_some();
    let same = record.owner.as_deref() == Some(owner_token);
    let lease_open = record.lease_until > clock;
    if same && lease_open {
        record.lease_until = clock.saturating_add(lease_ms);
    } else if lease_open {
        return Err(Error::new("execution is owned"));
    } else {
        record.owner = Some(owner_token.to_string());
        record.fence = record.fence.saturating_add(1);
        record.lease_until = clock.saturating_add(lease_ms);
    }
    if record.artifact_hash.is_empty() {
        record.artifact_hash = artifact_hash.to_string();
        record.handler_keys = keys;
    }
    Ok(StartedChild {
        parent_execution_id: link.parent_execution_id,
        invoke_id: link.invoke_id,
        program_name: link.program_name,
        arguments: link.arguments,
        revision: record.revision,
        fence: record.fence,
        lease_until: record.lease_until,
        initialized,
    })
}

pub(super) fn created(state: &State, execution_id: &str) -> Result<CreatedChild> {
    let record = state
        .executions
        .get(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    let Some(link) = &record.created else {
        return Err(Error::new("unknown execution"));
    };
    Ok(CreatedChild {
        parent_execution_id: link.parent_execution_id.clone(),
        invoke_id: link.invoke_id.clone(),
        child_execution_id: execution_id.to_string(),
        program_name: link.program_name.clone(),
        arguments: link.arguments.clone(),
    })
}

pub(super) fn pending(intents: &BTreeMap<String, StoredIntent>) -> Option<InvokeIntent> {
    intents
        .values()
        .find(|intent| !intent.committed)
        .map(|intent| InvokeIntent {
            invoke_id: intent.invoke_id.clone(),
            child_execution_id: intent.child_execution_id.clone(),
            program_name: intent.program_name.clone(),
            arguments: intent.arguments.clone(),
            digest: intent.digest.clone(),
        })
}

/// A checkpoint confirms every stored intent whose child id the continuation
/// represents. A join confirms each represented child and does not inspect
/// branch phase. A plain invoke still confirms the one pending child wait.
/// A terminal continuation, including cancellation, leaves every intent in place.
pub(super) fn consume(intents: &mut BTreeMap<String, StoredIntent>, continuation: &Continuation) {
    if is_terminal(continuation.status) {
        return;
    }
    for child_id in represented_children(continuation) {
        if let Some(intent) = intents.get_mut(&child_id) {
            intent.committed = true;
        }
    }
}

fn represented_children(continuation: &Continuation) -> Vec<String> {
    if let Some(join) = &continuation.join {
        return join
            .branches
            .iter()
            .filter_map(|branch| match &branch.op {
                Some(BranchOp::Child {
                    child_execution_id, ..
                }) => Some(child_execution_id.clone()),
                _ => None,
            })
            .collect();
    }
    match &continuation.pending {
        Some(PendingOp::Wait {
            kind: WaitKind::Child {
                child_execution_id, ..
            },
        }) => vec![child_execution_id.clone()],
        _ => Vec::new(),
    }
}
