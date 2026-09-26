use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::Value as Json;

use crate::error::LocalError;

pub struct ProductDb {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ProgramRow {
    pub id: String,
    pub artifact_json: String,
    pub artifact_hash: String,
    pub language: String,
    pub frontend_id: String,
    pub frontend_version: String,
    pub language_semantics_version: String,
    pub engine_format_version: i64,
    pub has_effects: bool,
    pub version_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct VersionRow {
    pub id: String,
    pub artifact_hash: String,
    pub language: String,
    pub frontend_id: String,
    pub frontend_version: String,
    pub language_semantics_version: String,
    pub engine_format_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct RecordRow {
    pub program_id: String,
    pub input_json: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ProductDb {
    pub fn open(path: &Path) -> Result<Self, LocalError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| LocalError::message(err.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", "5000")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), LocalError> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS trigora_records (
                id TEXT PRIMARY KEY,
                program_id TEXT NOT NULL,
                input_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS trigora_projects (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                name TEXT NOT NULL UNIQUE,
                slug TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS trigora_programs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                artifact_json TEXT NOT NULL,
                artifact_hash TEXT NOT NULL,
                language TEXT NOT NULL,
                frontend_id TEXT NOT NULL,
                frontend_version TEXT NOT NULL,
                language_semantics_version TEXT NOT NULL,
                engine_format_version INTEGER NOT NULL,
                has_effects INTEGER NOT NULL,
                version_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS trigora_program_versions (
                id TEXT PRIMARY KEY,
                program_id TEXT NOT NULL,
                artifact_hash TEXT NOT NULL,
                language TEXT NOT NULL,
                frontend_id TEXT NOT NULL,
                frontend_version TEXT NOT NULL,
                language_semantics_version TEXT NOT NULL,
                engine_format_version INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )?;
        let seeded: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM trigora_projects WHERE id = 'default'",
                [],
                |row| row.get(0),
            )
            .ok();
        if seeded.is_none() {
            self.conn.execute(
                "INSERT INTO trigora_projects(id, workspace_id, name, slug, created_at)
                 VALUES ('default', 'local', 'default', 'default', ?1)",
                params![crate::now()],
            )?;
        }
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRow>, LocalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, slug, created_at FROM trigora_projects ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRow {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LocalError::from)
    }

    pub fn insert_project(&self, row: &ProjectRow) -> Result<(), LocalError> {
        self.conn.execute(
            "INSERT INTO trigora_projects(id, workspace_id, name, slug, created_at)
             VALUES (?1, 'local', ?2, ?3, ?4)",
            params![row.id, row.name, row.slug, row.created_at],
        )?;
        Ok(())
    }

    pub fn project_name_taken(&self, name: &str, slug: &str) -> Result<bool, LocalError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM trigora_projects WHERE name = ?1 OR slug = ?2",
            params![name, slug],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn upsert_program(&self, row: &ProgramRow) -> Result<(), LocalError> {
        self.conn.execute(
            "INSERT INTO trigora_programs(
                id, name, artifact_json, artifact_hash, language, frontend_id, frontend_version,
                language_semantics_version, engine_format_version, has_effects, version_id,
                created_at, updated_at
             ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                artifact_json = excluded.artifact_json,
                artifact_hash = excluded.artifact_hash,
                language = excluded.language,
                frontend_id = excluded.frontend_id,
                frontend_version = excluded.frontend_version,
                language_semantics_version = excluded.language_semantics_version,
                engine_format_version = excluded.engine_format_version,
                has_effects = excluded.has_effects,
                version_id = excluded.version_id,
                updated_at = excluded.updated_at",
            params![
                row.id,
                row.artifact_json,
                row.artifact_hash,
                row.language,
                row.frontend_id,
                row.frontend_version,
                row.language_semantics_version,
                row.engine_format_version,
                row.has_effects as i64,
                row.version_id,
                row.created_at,
                row.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn program_created_at(&self, id: &str) -> Result<Option<String>, LocalError> {
        let value = self
            .conn
            .query_row(
                "SELECT created_at FROM trigora_programs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();
        Ok(value)
    }

    pub fn list_programs(&self) -> Result<Vec<ProgramRow>, LocalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, artifact_json, artifact_hash, language, frontend_id, frontend_version,
                    language_semantics_version, engine_format_version, has_effects, version_id,
                    created_at, updated_at
             FROM trigora_programs",
        )?;
        let rows = stmt.query_map([], read_program)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LocalError::from)
    }

    pub fn insert_version(&self, program_id: &str, row: &VersionRow) -> Result<(), LocalError> {
        self.conn.execute(
            "INSERT INTO trigora_program_versions(
                id, program_id, artifact_hash, language, frontend_id, frontend_version,
                language_semantics_version, engine_format_version, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                program_id,
                row.artifact_hash,
                row.language,
                row.frontend_id,
                row.frontend_version,
                row.language_semantics_version,
                row.engine_format_version,
                row.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn versions(&self, program_id: &str) -> Result<Vec<VersionRow>, LocalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, artifact_hash, language, frontend_id, frontend_version,
                    language_semantics_version, engine_format_version, created_at
             FROM trigora_program_versions WHERE program_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![program_id], |row| {
            Ok(VersionRow {
                id: row.get(0)?,
                artifact_hash: row.get(1)?,
                language: row.get(2)?,
                frontend_id: row.get(3)?,
                frontend_version: row.get(4)?,
                language_semantics_version: row.get(5)?,
                engine_format_version: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LocalError::from)
    }

    pub fn insert_record(
        &self,
        id: &str,
        program_id: &str,
        input: &Json,
        created_at: &str,
    ) -> Result<(), LocalError> {
        self.conn.execute(
            "INSERT INTO trigora_records(id, program_id, input_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, program_id, input.to_string(), created_at],
        )?;
        Ok(())
    }

    pub fn touch_record(&self, id: &str, updated_at: &str) -> Result<(), LocalError> {
        self.conn.execute(
            "UPDATE trigora_records SET updated_at = ?1 WHERE id = ?2",
            params![updated_at, id],
        )?;
        Ok(())
    }

    pub fn record(&self, id: &str) -> Result<Option<RecordRow>, LocalError> {
        let row = self
            .conn
            .query_row(
                "SELECT program_id, input_json, created_at, updated_at FROM trigora_records WHERE id = ?1",
                params![id],
                |row| {
                    Ok(RecordRow {
                        program_id: row.get(0)?,
                        input_json: row.get(1)?,
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                },
            )
            .ok();
        Ok(row)
    }

    pub fn execution_rows(&self) -> Result<Vec<(String, String, String)>, LocalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, artifact_hash, status FROM execution")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LocalError::from)
    }
}

fn read_program(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProgramRow> {
    let has_effects: i64 = row.get(8)?;
    Ok(ProgramRow {
        id: row.get(0)?,
        artifact_json: row.get(1)?,
        artifact_hash: row.get(2)?,
        language: row.get(3)?,
        frontend_id: row.get(4)?,
        frontend_version: row.get(5)?,
        language_semantics_version: row.get(6)?,
        engine_format_version: row.get(7)?,
        has_effects: has_effects != 0,
        version_id: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}
