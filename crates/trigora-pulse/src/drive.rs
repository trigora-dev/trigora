use std::collections::BTreeMap;

use tcc_core::{EffectStatus, Engine, EngineOutcome, HostRequest, HostResponse};
use tcc_state::{decode_value, encode_continuation, encode_value, Value};

use crate::authority::{Commit, JournalRow};
use crate::error::{Error, Result};

const BUDGET: u32 = 65_536;

pub(crate) struct Memory {
    pub journal: BTreeMap<String, JournalRow>,
    pub revision: u64,
    pub last_effect: Option<LastEffect>,
}

#[derive(Clone)]
pub(crate) struct LastEffect {
    pub key: String,
    pub input: String,
}

pub(crate) enum Answer {
    Value(String),
    Failed(String),
    TimedOut,
    Disconnected,
}

pub(crate) enum Step {
    NeedEffect,
    TimedOut,
    Disconnected,
    Commit(Commit),
    Suspended,
    Finished,
    Failed(String),
}

pub(crate) struct Turn<'a> {
    pub engine: &'a mut Engine,
    pub memory: &'a mut Memory,
    pub execution_id: String,
    pub owner_token: String,
    pub answer: Option<Answer>,
    pub confirm: Option<std::result::Result<u64, String>>,
}

pub(crate) fn turn(turn: Turn<'_>) -> Result<Step> {
    // The stock `tcc_host::Driver` returns on RegisterTimer before PersistCheckpoint.
    let _stock_host = std::any::type_name::<tcc_host::MemoryHost>();
    let _ = _stock_host;

    let Turn {
        engine,
        memory,
        execution_id,
        owner_token,
        answer,
        confirm,
    } = turn;

    if let Some(confirm) = confirm {
        match confirm {
            Ok(revision) => {
                engine
                    .apply_host_response(HostResponse::PersistConfirmed { revision })
                    .map_err(|err| Error::new(err.to_string()))?;
                memory.revision = revision;
            }
            Err(message) => return Ok(Step::Failed(message)),
        }
    }

    if let Some(answer) = answer {
        match answer {
            Answer::TimedOut => return Ok(Step::TimedOut),
            Answer::Disconnected => return Ok(Step::Disconnected),
            Answer::Failed(message) => {
                engine
                    .apply_host_response(HostResponse::EffectFailed { message })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
            Answer::Value(encoded) => {
                let value = decode_value(encoded.as_bytes())
                    .map_err(|err| Error::new(format!("effect value: {err}")))?;
                engine
                    .apply_host_response(HostResponse::EffectResult { value })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
        }
    }

    loop {
        match engine.run_until_host(BUDGET) {
            EngineOutcome::Host(request) => match request {
                HostRequest::RunEffect {
                    key,
                    idempotency_key: _,
                    input,
                } => {
                    let encoded = encode_value(&input)
                        .map_err(|err| Error::new(format!("effect input: {err}")))?;
                    let encoded = String::from_utf8(encoded)
                        .map_err(|_| Error::new("effect input is not utf-8"))?;
                    if let Some(row) = memory.journal.get(&key) {
                        if row.input != encoded {
                            return Ok(Step::Failed(format!(
                                "effect {key} input does not match the journal"
                            )));
                        }
                        if row.status == "completed" {
                            let value = decode_value(row.result.as_bytes())
                                .map_err(|err| Error::new(format!("journal value: {err}")))?;
                            engine
                                .apply_host_response(HostResponse::EffectResult { value })
                                .map_err(|err| Error::new(err.to_string()))?;
                            continue;
                        }
                        if row.status == "failed" {
                            engine
                                .apply_host_response(HostResponse::EffectFailed {
                                    message: row.result.clone(),
                                })
                                .map_err(|err| Error::new(err.to_string()))?;
                            continue;
                        }
                    }
                    memory.last_effect = Some(LastEffect {
                        key: key.clone(),
                        input: encoded,
                    });
                    return Ok(Step::NeedEffect);
                }
                HostRequest::PersistEffect { record } => {
                    let Some(last) = memory.last_effect.as_ref() else {
                        return Ok(Step::Failed("effect journal is missing its input".into()));
                    };
                    if last.key != record.key {
                        return Ok(Step::Failed("effect journal key mismatch".into()));
                    }
                    let result = match &record.result {
                        Some(value) => encode_owned(value)?,
                        None => encode_owned(&Value::Undefined)?,
                    };
                    let status = match record.status {
                        EffectStatus::Completed => "completed",
                        EffectStatus::Failed => "failed",
                        EffectStatus::Started => "started",
                    };
                    memory.journal.insert(
                        record.key.clone(),
                        JournalRow {
                            key: record.key,
                            idempotency_key: record.idempotency_key,
                            status: status.to_string(),
                            input: last.input.clone(),
                            result,
                        },
                    );
                    engine
                        .apply_host_response(HostResponse::Ack)
                        .map_err(|err| Error::new(err.to_string()))?;
                }
                HostRequest::RegisterTimer { .. } | HostRequest::RegisterWait { .. } => {
                    engine
                        .apply_host_response(HostResponse::Ack)
                        .map_err(|err| Error::new(err.to_string()))?;
                }
                HostRequest::CreateChild { .. } => {
                    return Ok(Step::Failed(
                        "create_child is not available in this Pulse host".into(),
                    ));
                }
                HostRequest::FetchArtifact { hash } => {
                    engine
                        .apply_host_response(HostResponse::Artifact { hash })
                        .map_err(|err| Error::new(err.to_string()))?;
                }
                HostRequest::PersistCheckpoint { revision, .. } => {
                    let mut snapshot = engine.continuation().clone();
                    if snapshot.revision != memory.revision || revision != snapshot.revision + 1 {
                        return Ok(Step::Failed(format!(
                            "engine revision {revision} does not follow {}",
                            memory.revision
                        )));
                    }
                    snapshot.revision = revision;
                    let bytes = encode_continuation(&snapshot)
                        .map_err(|err| Error::new(format!("encode continuation: {err}")))?;
                    return Ok(Step::Commit(Commit {
                        execution_id,
                        owner_token,
                        expected_revision: memory.revision,
                        continuation: bytes,
                        journal: memory.journal.clone(),
                        timers: None,
                        waits: None,
                    }));
                }
            },
            EngineOutcome::Completed { .. } => return Ok(Step::Finished),
            EngineOutcome::Failed { message } => return Ok(Step::Failed(message)),
            EngineOutcome::Cancelled => return Ok(Step::Failed("execution cancelled".into())),
            EngineOutcome::BudgetExhausted => {
                return Ok(Step::Failed("instruction budget exhausted".into()))
            }
            EngineOutcome::Suspended => return Ok(Step::Suspended),
        }
    }
}

fn encode_owned(value: &Value) -> Result<String> {
    let bytes = encode_value(value).map_err(|err| Error::new(format!("encode value: {err}")))?;
    String::from_utf8(bytes).map_err(|_| Error::new("value is not utf-8"))
}
