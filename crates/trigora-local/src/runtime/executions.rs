use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value as Json};
use tcc_host_sqlite::{resume_execution, start_execution_with_args, SqliteHost, Store};
use tcc_state::{decode_continuation, PendingOp, WaitKind};

use super::programs::not_found_program;
use super::LocalRuntime;
use crate::error::LocalError;
use crate::value::{argument_vector, continuation_result, plain_to_value};

impl LocalRuntime {
    pub fn start(&self, body: &Json) -> Result<Json, LocalError> {
        let program_id = body.get("programId").and_then(Json::as_str).unwrap_or("");
        if program_id.is_empty() {
            return Err(LocalError::invalid("`programId` is required."));
        }
        let program = self
            .catalog
            .lock()
            .expect("catalog")
            .get(program_id)
            .ok_or_else(|| not_found_program(program_id))?;
        if program.blocks_execution {
            return Err(LocalError::invalid(
                "Local deploy execution does not support deployed effect bundles yet.",
            ));
        }
        let input_present = body.get("input");
        let stored_input = input_present.cloned().unwrap_or(Json::Null);
        let args = argument_vector(input_present);
        let id = format!("exec_local_{}", uuid::Uuid::new_v4());
        let created_at = crate::now();
        self.product.lock().expect("product").insert_record(
            &id,
            program_id,
            &stored_input,
            &created_at,
        )?;
        let child_artifacts = self.child_artifacts();
        self.emit(json!({
            "type": "started",
            "execution": {
                "id": id,
                "programId": program_id,
                "status": "running",
            }
        }));
        let drive = self.drive_lock(&id);
        let _held = drive.lock().expect("drive");
        let mut host = self.open_host(&id)?;
        host.auto_deliver = false;
        host.cancel = false;
        host.child_artifacts = child_artifacts;
        self.install_provider(&mut host, program_id, &id);
        start_execution_with_args(&mut host, &program.artifact_json, &id, &args)?;
        drop(host);
        drop(_held);
        self.touch(&id)?;
        let execution = self.execution_json(&id)?;
        self.emit_terminal(&execution, true);
        Ok(json!({ "execution": execution }))
    }

    pub fn send(&self, id: &str, body: &Json) -> Result<Json, LocalError> {
        let name = body.get("name").and_then(Json::as_str).unwrap_or("");
        if name.is_empty() {
            return Err(LocalError::invalid("`name` is required."));
        }
        let current = self.execution_json(id)?;
        let status = current.get("status").and_then(Json::as_str).unwrap_or("");
        let wait = current.get("wait");
        let wait_type = wait
            .and_then(|item| item.get("type"))
            .and_then(Json::as_str);
        let event = wait
            .and_then(|item| item.get("event"))
            .and_then(Json::as_str);
        if status != "waiting" || wait_type != Some("event") {
            return Err(LocalError::new(
                409,
                "execution_not_waiting",
                format!("Execution \"{id}\" is not waiting for an event."),
            ));
        }
        if event != Some(name) {
            return Err(LocalError::new(
                409,
                "event_mismatch",
                format!(
                    "Execution \"{id}\" is waiting for event \"{}\".",
                    event.unwrap_or("")
                ),
            ));
        }
        let payload = body
            .get("payload")
            .cloned()
            .unwrap_or(Json::Object(Default::default()));
        let program_id = current
            .get("programId")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        self.emit(json!({
            "type": "resumed",
            "execution": { "id": id, "programId": program_id, "status": "running" }
        }));
        let drive = self.drive_lock(id);
        let _held = drive.lock().expect("drive");
        let mut host = self.open_host(id)?;
        host.auto_deliver = true;
        host.event_payload = Some(plain_to_value(&payload));
        host.cancel = false;
        host.child_artifacts = self.child_artifacts();
        self.install_provider(&mut host, &program_id, id);
        resume_execution(&mut host, None, id)?;
        drop(host);
        drop(_held);
        self.touch(id)?;
        let execution = self.execution_json(id)?;
        self.emit_terminal(&execution, false);
        Ok(json!({ "execution": execution }))
    }

    pub fn cancel(&self, id: &str) -> Result<Json, LocalError> {
        let current = self.execution_json(id)?;
        let status = current.get("status").and_then(Json::as_str).unwrap_or("");
        if status == "completed" || status == "failed" {
            return Err(LocalError::new(
                409,
                "execution_not_cancellable",
                format!("Execution \"{id}\" is already {status}."),
            ));
        }
        if status == "cancelled" {
            return Ok(json!({ "execution": current }));
        }
        let program_id = current
            .get("programId")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let drive = self.drive_lock(id);
        let _held = drive.lock().expect("drive");
        let mut host = self.open_host(id)?;
        host.auto_deliver = false;
        host.cancel = true;
        host.child_artifacts = self.child_artifacts();
        self.install_provider(&mut host, &program_id, id);
        resume_execution(&mut host, None, id)?;
        drop(host);
        drop(_held);
        self.touch(id)?;
        let execution = self.execution_json(id)?;
        self.emit_terminal(&execution, false);
        Ok(json!({ "execution": execution }))
    }

    pub fn get_execution(&self, id: &str) -> Result<Json, LocalError> {
        Ok(json!({ "execution": self.execution_json(id)? }))
    }

    pub fn list_executions(&self) -> Result<Json, LocalError> {
        let ids = self.product.lock().expect("product").execution_rows()?;
        let mut executions = Vec::new();
        for (id, _, _) in ids {
            if let Ok(execution) = self.execution_json(&id) {
                executions.push(execution);
            }
        }
        executions.sort_by(|left, right| {
            right
                .get("updatedAt")
                .and_then(Json::as_str)
                .unwrap_or("")
                .cmp(left.get("updatedAt").and_then(Json::as_str).unwrap_or(""))
        });
        Ok(json!({ "executions": executions }))
    }

    pub fn result(&self, id: &str) -> Result<Json, LocalError> {
        let execution = self.execution_json(id)?;
        Ok(json!({
            "result": {
                "status": execution.get("status").cloned().unwrap_or(Json::Null),
                "result": execution.get("result").cloned().unwrap_or(Json::Null),
                "error": execution.get("error").cloned().unwrap_or(Json::Null),
            }
        }))
    }

    fn execution_json(&self, id: &str) -> Result<Json, LocalError> {
        let store = Store::open(&self.db_path).map_err(|err| LocalError::message(err.message))?;
        let row = store
            .execution(id)
            .map_err(|err| LocalError::message(err.message))?
            .ok_or_else(|| {
                LocalError::new(
                    404,
                    "execution_not_found",
                    format!("Execution \"{id}\" was not found."),
                )
            })?;
        let record = self.product.lock().expect("product").record(id)?;
        let saved = store
            .checkpoint(id)
            .map_err(|err| LocalError::message(err.message))?;
        let continuation = saved
            .as_ref()
            .map(|saved| decode_continuation(saved.body.as_bytes()))
            .transpose()?;
        let status = map_status(&row.status);
        let result = continuation.as_ref().and_then(continuation_result);
        let failed_message = continuation
            .as_ref()
            .and_then(|item| item.result.as_ref())
            .and_then(|value| match value {
                tcc_state::Value::String(text) => Some(text.clone()),
                _ => None,
            });
        let wait = if status == "waiting" {
            execution_wait(continuation.as_ref(), &store, id)
        } else {
            None
        };
        let program_id = record
            .as_ref()
            .map(|record| record.program_id.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let input = record
            .as_ref()
            .and_then(|record| serde_json::from_str(&record.input_json).ok())
            .unwrap_or(Json::Null);
        let created_at = record
            .as_ref()
            .map(|record| record.created_at.clone())
            .unwrap_or_else(crate::now);
        let updated_at = record
            .as_ref()
            .map(|record| record.updated_at.clone())
            .unwrap_or_else(crate::now);
        let mut body = json!({
            "id": id,
            "projectId": "default",
            "programId": program_id,
            "programName": program_id,
            "programVersionId": if row.artifact_hash.is_empty() { id.to_string() } else { row.artifact_hash.clone() },
            "artifactHash": row.artifact_hash,
            "engineFormatVersion": 1,
            "status": status,
            "input": input,
            "attempt": 1,
            "createdAt": created_at,
            "updatedAt": updated_at,
        });
        if status == "completed" {
            if let Some(result) = result {
                body["result"] = result;
            }
        }
        if status == "failed" {
            body["error"] = json!({
                "name": "Error",
                "message": failed_message.unwrap_or_else(|| "Execution failed.".to_string()),
            });
        }
        if let Some(wait) = wait {
            body["wait"] = wait;
        }
        Ok(body)
    }

    pub fn restored_waiting(&self) -> Result<usize, LocalError> {
        let rows = self.product.lock().expect("product").execution_rows()?;
        Ok(rows
            .iter()
            .filter(|(_, _, status)| status == "suspended")
            .count())
    }

    fn touch(&self, id: &str) -> Result<(), LocalError> {
        self.product
            .lock()
            .expect("product")
            .touch_record(id, &crate::now())
    }

    fn child_artifacts(&self) -> HashMap<String, String> {
        self.catalog
            .lock()
            .expect("catalog")
            .list()
            .into_iter()
            .map(|(id, program)| (id, program.artifact_json))
            .collect()
    }

    fn open_host(&self, execution_id: &str) -> Result<SqliteHost, LocalError> {
        SqliteHost::open(&self.db_path, execution_id).map_err(LocalError::from)
    }

    fn drive_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.drives
            .lock()
            .expect("drives")
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn emit(&self, event: Json) {
        (self.events)(&event);
    }

    fn emit_terminal(&self, execution: &Json, include_waiting: bool) {
        let status = execution.get("status").and_then(Json::as_str).unwrap_or("");
        let kind = match status {
            "waiting" if include_waiting => "waiting",
            "completed" => "completed",
            "failed" => "failed",
            "cancelled" => "cancelled",
            _ => return,
        };
        self.emit(json!({ "type": kind, "execution": execution }));
    }
}

fn execution_wait(
    continuation: Option<&tcc_state::Continuation>,
    store: &Store,
    id: &str,
) -> Option<Json> {
    if let Some(PendingOp::Wait { kind }) = continuation.and_then(|item| item.pending.as_ref()) {
        return Some(match kind {
            WaitKind::Event { event_name, .. } => json!({ "type": "event", "event": event_name }),
            WaitKind::Timer { resume_at_ms } => json!({
                "type": "timer",
                "wakeAt": wake_at(*resume_at_ms),
            }),
            WaitKind::Child {
                child_execution_id, ..
            } => json!({ "type": "child", "executionId": child_execution_id }),
        });
    }
    if let Ok(Some(wait)) = store.pending_wait(id) {
        return Some(json!({ "type": "event", "event": wait.event_name }));
    }
    if let Ok(Some(timer)) = store.pending_timer(id, None) {
        return Some(json!({
            "type": "timer",
            "wakeAt": wake_at(timer.wake_at_ms.max(0) as u64),
        }));
    }
    None
}

fn wake_at(millis: u64) -> String {
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

fn map_status(status: &str) -> &'static str {
    match status {
        "suspended" => "waiting",
        "runnable" => "running",
        "completed" | "failed" | "cancelled" => {
            if status == "completed" {
                "completed"
            } else if status == "failed" {
                "failed"
            } else {
                "cancelled"
            }
        }
        _ => "running",
    }
}
