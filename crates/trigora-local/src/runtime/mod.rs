use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value as Json;
use tcc_host_sqlite::Store;

use crate::error::LocalError;
use crate::product::ProductDb;
use programs::registered_from_row;

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

mod effects;
mod executions;
mod programs;

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
}
