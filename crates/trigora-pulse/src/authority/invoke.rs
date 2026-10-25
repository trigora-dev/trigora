use tcc_state::{Continuation, PendingOp, WaitKind};

use super::identity::{is_terminal, sha256};
use super::state::{CreatedLink, State, StoredIntent};
use super::types::{CreatedChild, InvokeIntent, InvokeReceipt, InvokeSpec};
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
    let same = record.intent.as_ref().is_some_and(|intent| {
        intent.invoke_id == spec.invoke_id
            && intent.child_execution_id == spec.child_execution_id
            && intent.program_name == spec.program_name
            && intent.digest == digest
    });
    if same {
        return Ok(InvokeReceipt {
            revision: record.revision,
            idempotent: true,
        });
    }
    if record
        .intent
        .as_ref()
        .is_some_and(|intent| !intent.committed)
    {
        return Err(Error::new("invoke conflict"));
    }
    record.intent = Some(StoredIntent {
        invoke_id: spec.invoke_id.clone(),
        child_execution_id: spec.child_execution_id.clone(),
        program_name: spec.program_name.clone(),
        arguments: spec.arguments.clone(),
        digest,
        committed: false,
    });
    Ok(InvokeReceipt {
        revision: record.revision,
        idempotent: false,
    })
}

/// Create the child named by the parent's stored intent. The parent's lease is
/// not consulted. An offered spec that differs from that intent conflicts and
/// writes nothing.
pub(super) fn ensure(
    state: &mut State,
    parent_id: &str,
    offered: Option<&InvokeSpec>,
) -> Result<InvokeReceipt> {
    let (revision, child_id, link) = {
        let parent = state
            .executions
            .get(parent_id)
            .ok_or_else(|| Error::new("unknown execution"))?;
        let Some(intent) = parent.intent.as_ref() else {
            return Err(Error::new("invoke intent is missing"));
        };
        if let Some(offered) = offered {
            if offered.invoke_id != intent.invoke_id
                || offered.child_execution_id != intent.child_execution_id
                || offered.program_name != intent.program_name
                || offered.arguments != intent.arguments
            {
                return Err(Error::new("child conflict"));
            }
        }
        if intent.child_execution_id == parent_id {
            return Err(Error::new("child conflict"));
        }
        (
            parent.revision,
            intent.child_execution_id.clone(),
            CreatedLink {
                parent_execution_id: parent_id.to_string(),
                invoke_id: intent.invoke_id.clone(),
                program_name: intent.program_name.clone(),
                arguments: intent.arguments.clone(),
                digest: intent.digest.clone(),
            },
        )
    };
    if let Some(existing) = state.executions.get(&child_id) {
        if existing.created.as_ref().is_some_and(|created| {
            created.parent_execution_id == link.parent_execution_id
                && created.invoke_id == link.invoke_id
                && created.program_name == link.program_name
                && created.arguments == link.arguments
                && created.digest == link.digest
        }) {
            return Ok(InvokeReceipt {
                revision,
                idempotent: true,
            });
        }
        return Err(Error::new("child conflict"));
    }
    let mut record = super::state::Record::new(String::new(), 0, 0);
    record.owner = None;
    record.created = Some(link);
    state.executions.insert(child_id, record);
    Ok(InvokeReceipt {
        revision,
        idempotent: false,
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

pub(super) fn pending(intent: &Option<StoredIntent>) -> Option<InvokeIntent> {
    intent
        .as_ref()
        .filter(|intent| !intent.committed)
        .map(|intent| InvokeIntent {
            invoke_id: intent.invoke_id.clone(),
            child_execution_id: intent.child_execution_id.clone(),
            program_name: intent.program_name.clone(),
            arguments: intent.arguments.clone(),
            digest: intent.digest.clone(),
        })
}

/// The parent checkpoint that waits on this child consumes the intent. A
/// terminal continuation, including cancellation, leaves it in place.
pub(super) fn consume(intent: &mut Option<StoredIntent>, continuation: &Continuation) {
    if is_terminal(continuation.status) {
        return;
    }
    let Some(stored) = intent.as_mut() else {
        return;
    };
    if stored.committed {
        return;
    }
    let Some(PendingOp::Wait {
        kind: WaitKind::Child {
            invoke_id,
            child_execution_id,
        },
    }) = &continuation.pending
    else {
        return;
    };
    if invoke_id == &stored.invoke_id && child_execution_id == &stored.child_execution_id {
        stored.committed = true;
    }
}
