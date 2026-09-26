use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};
use tcc_host::HostError;
use tcc_host_sqlite::{resume_execution, start_execution_with_args, SqliteHost, Store};
use tcc_state::{decode_continuation, PendingOp, WaitKind};

use crate::error::LocalError;
use crate::product::{ProductDb, ProgramRow, ProjectRow, VersionRow};
use crate::value::{argument_vector, continuation_result, plain_to_value, value_to_plain};

#[derive(Clone)]
pub struct EffectEndpoint {
    pub url: String,
    pub secret: String,
}

pub type EventSink = Arc<dyn Fn(&Json) + Send + Sync>;

#[derive(Clone)]
struct RegisteredProgram {
    artifact_json: String,
    artifact_hash: String,
    language: String,
    frontend_id: String,
    frontend_version: String,
    language_semantics_version: String,
    engine_format_version: i64,
    blocks_execution: bool,
    #[allow(dead_code)]
    deployed: bool,
    version_id: String,
    created_at: String,
    updated_at: String,
}

struct Catalog {
    files: HashMap<String, RegisteredProgram>,
    deployed: HashMap<String, RegisteredProgram>,
}

impl Catalog {
    fn get(&self, id: &str) -> Option<RegisteredProgram> {
        self.files
            .get(id)
            .or_else(|| self.deployed.get(id))
            .cloned()
    }

    fn list(&self) -> Vec<(String, RegisteredProgram)> {
        let mut rows: Vec<_> = self
            .files
            .iter()
            .map(|(id, program)| (id.clone(), program.clone()))
            .collect();
        for (id, program) in &self.deployed {
            if !self.files.contains_key(id) {
                rows.push((id.clone(), program.clone()));
            }
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        rows
    }
}

pub struct LocalRuntime {
    db_path: PathBuf,
    product: Mutex<ProductDb>,
    catalog: Mutex<Catalog>,
    drives: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    effect: Option<EffectEndpoint>,
    events: EventSink,
}

#[derive(Clone)]
pub struct FileProgram {
    pub id: String,
    pub artifact_json: String,
    pub artifact_hash: String,
    pub language: String,
    pub frontend_id: String,
    pub frontend_version: String,
    pub language_semantics_version: String,
    pub engine_format_version: i64,
}

impl LocalRuntime {
    pub fn open(
        db_path: impl Into<PathBuf>,
        effect: Option<EffectEndpoint>,
        events: EventSink,
    ) -> Result<Self, LocalError> {
        let db_path = db_path.into();
        let _host = Store::open(&db_path).map_err(|err| LocalError::message(err.message))?;
        let product = ProductDb::open(&db_path)?;
        let mut deployed = HashMap::new();
        for row in product.list_programs()? {
            deployed.insert(row.id.clone(), registered_from_row(row));
        }
        Ok(Self {
            db_path,
            product: Mutex::new(product),
            catalog: Mutex::new(Catalog {
                files: HashMap::new(),
                deployed,
            }),
            drives: Mutex::new(HashMap::new()),
            effect,
            events,
        })
    }

    pub fn replace_files(&self, programs: Vec<FileProgram>) -> Result<(), LocalError> {
        let now = crate::now();
        let mut files = HashMap::new();
        for program in programs {
            files.insert(
                program.id.clone(),
                RegisteredProgram {
                    artifact_json: program.artifact_json,
                    artifact_hash: program.artifact_hash,
                    language: program.language,
                    frontend_id: program.frontend_id,
                    frontend_version: program.frontend_version,
                    language_semantics_version: program.language_semantics_version,
                    engine_format_version: program.engine_format_version,
                    blocks_execution: false,
                    deployed: false,
                    version_id: String::new(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            );
        }
        self.catalog.lock().expect("catalog").files = files;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Json, LocalError> {
        let projects = self.product.lock().expect("product").list_projects()?;
        Ok(json!({
            "projects": projects.into_iter().map(|project| json!({
                "id": project.id,
                "name": project.name,
                "slug": project.slug,
            })).collect::<Vec<_>>()
        }))
    }

    pub fn create_project(&self, body: &Json) -> Result<Json, LocalError> {
        let name = body.get("name").and_then(Json::as_str).unwrap_or("").trim();
        if name.is_empty() {
            return Err(LocalError::invalid("`name` is required."));
        }
        let requested = body.get("slug").and_then(Json::as_str).unwrap_or("");
        let slug = slugify(if requested.trim().is_empty() {
            name
        } else {
            requested.trim()
        });
        if slug.is_empty() {
            return Err(LocalError::invalid("`name` is required."));
        }
        let product = self.product.lock().expect("product");
        if product.project_name_taken(name, &slug)? {
            return Err(LocalError::new(
                409,
                "conflict",
                format!("Project \"{name}\" already exists."),
            ));
        }
        let created_at = crate::now();
        let row = ProjectRow {
            id: format!("prj_{}", uuid::Uuid::new_v4()),
            name: name.to_string(),
            slug: slug.clone(),
            created_at: created_at.clone(),
        };
        product.insert_project(&row)?;
        Ok(json!({
            "project": {
                "id": row.id,
                "workspaceId": "local",
                "name": row.name,
                "slug": row.slug,
                "createdAt": created_at,
            }
        }))
    }

    pub fn deploy(&self, body: &Json) -> Result<Json, LocalError> {
        let name = body.get("name").and_then(Json::as_str).unwrap_or("").trim();
        let artifact = body.get("artifact");
        let blob = artifact
            .and_then(|item| item.get("blob"))
            .and_then(Json::as_str);
        let hash = artifact
            .and_then(|item| item.get("hash"))
            .and_then(Json::as_str);
        if name.is_empty() || blob.is_none() || hash.is_none() {
            return Err(LocalError::invalid("`name` and `artifact` are required."));
        }
        let language = body
            .get("effectBundle")
            .and_then(|item| item.get("language"))
            .and_then(Json::as_str)
            .unwrap_or("");
        if language != "typescript" && language != "python" && language != "rust" {
            return Err(LocalError::invalid("`effectBundle.language` is required."));
        }
        let files = body
            .get("effectBundle")
            .and_then(|item| item.get("files"))
            .and_then(Json::as_array);
        let has_effects = files.is_some_and(|files| !files.is_empty());
        let created_at = crate::now();
        let version_id = format!("ver_{}", uuid::Uuid::new_v4());
        let artifact = artifact.expect("checked");
        let product = self.product.lock().expect("product");
        let stored_created = product
            .program_created_at(name)?
            .unwrap_or_else(|| created_at.clone());
        let row = ProgramRow {
            id: name.to_string(),
            artifact_json: blob.expect("checked").to_string(),
            artifact_hash: hash.expect("checked").to_string(),
            language: language.to_string(),
            frontend_id: json_string(artifact, "frontendId"),
            frontend_version: json_string(artifact, "frontendVersion"),
            language_semantics_version: json_string(artifact, "languageSemanticsVersion"),
            engine_format_version: artifact
                .get("engineFormatVersion")
                .and_then(Json::as_i64)
                .unwrap_or(1),
            has_effects,
            version_id: version_id.clone(),
            created_at: stored_created.clone(),
            updated_at: created_at.clone(),
        };
        product.upsert_program(&row)?;
        product.insert_version(
            name,
            &VersionRow {
                id: version_id.clone(),
                artifact_hash: row.artifact_hash.clone(),
                language: row.language.clone(),
                frontend_id: row.frontend_id.clone(),
                frontend_version: row.frontend_version.clone(),
                language_semantics_version: row.language_semantics_version.clone(),
                engine_format_version: row.engine_format_version,
                created_at: created_at.clone(),
            },
        )?;
        drop(product);
        self.catalog.lock().expect("catalog").deployed.insert(
            name.to_string(),
            RegisteredProgram {
                artifact_json: row.artifact_json.clone(),
                artifact_hash: row.artifact_hash.clone(),
                language: row.language.clone(),
                frontend_id: row.frontend_id.clone(),
                frontend_version: row.frontend_version.clone(),
                language_semantics_version: row.language_semantics_version.clone(),
                engine_format_version: row.engine_format_version,
                blocks_execution: has_effects,
                deployed: true,
                version_id,
                created_at: stored_created,
                updated_at: created_at,
            },
        );
        let program = self.program_json(name)?;
        let version = program
            .get("program")
            .and_then(|item| item.get("currentVersion"))
            .cloned()
            .unwrap_or(Json::Null);
        Ok(json!({ "program": program["program"], "version": version }))
    }

    pub fn list_programs(&self) -> Result<Json, LocalError> {
        let catalog = self.catalog.lock().expect("catalog");
        let programs = catalog
            .list()
            .into_iter()
            .map(|(id, program)| {
                json!({
                    "id": id,
                    "name": id,
                    "language": program.language,
                    "currentVersionId": version_id(&program),
                    "updatedAt": program.updated_at,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({ "programs": programs }))
    }

    pub fn get_program(&self, id: &str) -> Result<Json, LocalError> {
        self.program_json(id)
    }

    pub fn list_versions(&self, id: &str) -> Result<Json, LocalError> {
        let program = self
            .catalog
            .lock()
            .expect("catalog")
            .get(id)
            .ok_or_else(|| not_found_program(id))?;
        let mut versions = self.product.lock().expect("product").versions(id)?;
        if versions.is_empty() {
            return Ok(json!({
                "versions": [{
                    "id": program.artifact_hash,
                    "artifactHash": program.artifact_hash,
                    "language": program.language,
                    "frontendId": program.frontend_id,
                    "frontendVersion": program.frontend_version,
                    "languageSemanticsVersion": program.language_semantics_version,
                    "createdAt": "1970-01-01T00:00:00.000Z",
                }]
            }));
        }
        let body = versions
            .drain(..)
            .map(|version| {
                json!({
                    "id": version.id,
                    "artifactHash": version.artifact_hash,
                    "language": version.language,
                    "frontendId": version.frontend_id,
                    "frontendVersion": version.frontend_version,
                    "languageSemanticsVersion": version.language_semantics_version,
                    "createdAt": version.created_at,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({ "versions": body }))
    }

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

    fn program_json(&self, id: &str) -> Result<Json, LocalError> {
        let program = self
            .catalog
            .lock()
            .expect("catalog")
            .get(id)
            .ok_or_else(|| not_found_program(id))?;
        let versions = self.product.lock().expect("product").versions(id)?;
        let latest = versions.last();
        let version = if let Some(latest) = latest {
            json!({
                "id": latest.id,
                "programId": id,
                "artifactHash": latest.artifact_hash,
                "engineFormatVersion": latest.engine_format_version,
                "languageSemanticsVersion": latest.language_semantics_version,
                "frontendId": latest.frontend_id,
                "frontendVersion": latest.frontend_version,
                "language": latest.language,
                "createdAt": latest.created_at,
            })
        } else {
            json!({
                "id": program.artifact_hash,
                "programId": id,
                "artifactHash": program.artifact_hash,
                "engineFormatVersion": program.engine_format_version,
                "languageSemanticsVersion": program.language_semantics_version,
                "frontendId": program.frontend_id,
                "frontendVersion": program.frontend_version,
                "language": program.language,
                "createdAt": program.updated_at,
            })
        };
        Ok(json!({
            "program": {
                "id": id,
                "projectId": "default",
                "name": id,
                "currentVersionId": version.get("id").cloned().unwrap_or(Json::Null),
                "currentVersion": version,
                "createdAt": program.created_at,
                "updatedAt": program.updated_at,
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

    fn install_provider(&self, host: &mut SqliteHost, program_id: &str, execution_id: &str) {
        let Some(endpoint) = self.effect.clone() else {
            return;
        };
        let program_id = program_id.to_string();
        let execution_id = execution_id.to_string();
        let events = Arc::clone(&self.events);
        host.set_effect_provider(Box::new(move |key, input| {
            events(&json!({
                "type": "effect",
                "name": key,
                "execution": {
                    "id": execution_id,
                    "programId": program_id,
                    "status": "running",
                }
            }));
            call_effect(&endpoint, &program_id, &execution_id, key, input)
        }));
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

fn call_effect(
    endpoint: &EffectEndpoint,
    program_id: &str,
    execution_id: &str,
    key: &str,
    input: &tcc_state::Value,
) -> Result<tcc_state::Value, HostError> {
    let body = json!({
        "programId": program_id,
        "executionId": execution_id,
        "key": key,
        "input": value_to_plain(input),
    })
    .to_string();
    let (host, port, path) = parse_http_url(&endpoint.url).map_err(HostError::Message)?;
    let mut stream = TcpStream::connect(format!("{host}:{port}"))
        .map_err(|err| HostError::Message(format!("effect callback failed: {err}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|err| HostError::Message(err.to_string()))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        endpoint.secret,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| HostError::Message(err.to_string()))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| HostError::Message(err.to_string()))?;
    let payload = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    let parsed: Json = serde_json::from_str(payload.trim()).map_err(|err| {
        HostError::Message(format!("effect callback returned invalid JSON: {err}"))
    })?;
    if let Some(error) = parsed.get("error").and_then(Json::as_str) {
        return Err(HostError::Message(error.to_string()));
    }
    Ok(plain_to_value(parsed.get("value").unwrap_or(&Json::Null)))
}

fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("effect callback URL must be http: {url}"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path}");
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| format!("effect callback URL is missing a port: {url}"))?;
    let port = port
        .parse()
        .map_err(|_| format!("effect callback port is invalid: {url}"))?;
    Ok((host.to_string(), port, path))
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

fn version_id(program: &RegisteredProgram) -> String {
    if program.version_id.is_empty() {
        program.artifact_hash.clone()
    } else {
        program.version_id.clone()
    }
}

fn registered_from_row(row: ProgramRow) -> RegisteredProgram {
    RegisteredProgram {
        artifact_json: row.artifact_json,
        artifact_hash: row.artifact_hash,
        language: row.language,
        frontend_id: row.frontend_id,
        frontend_version: row.frontend_version,
        language_semantics_version: row.language_semantics_version,
        engine_format_version: row.engine_format_version,
        blocks_execution: row.has_effects,
        deployed: true,
        version_id: row.version_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn not_found_program(id: &str) -> LocalError {
    LocalError::new(
        404,
        "program_not_found",
        format!("Program \"{id}\" was not found."),
    )
}

fn json_string(value: &Json, key: &str) -> String {
    value
        .get(key)
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}
