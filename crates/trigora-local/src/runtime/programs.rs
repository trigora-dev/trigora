use std::collections::HashMap;

use serde_json::{json, Value as Json};

use super::{FileProgram, LocalRuntime, RegisteredProgram};
use crate::error::LocalError;
use crate::product::{ProgramRow, ProjectRow, VersionRow};

impl LocalRuntime {
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
}

fn version_id(program: &RegisteredProgram) -> String {
    if program.version_id.is_empty() {
        program.artifact_hash.clone()
    } else {
        program.version_id.clone()
    }
}

pub(super) fn registered_from_row(row: ProgramRow) -> RegisteredProgram {
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

pub(in crate::runtime) fn not_found_program(id: &str) -> LocalError {
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
