use serde_json::json;
use tcc_ir::{encode_artifact, Artifact};

use super::common::*;

#[test]
fn seeds_the_default_project() {
    let path = temp_db();
    let runtime = runtime(&path);
    let projects = runtime.list_projects().unwrap();
    assert_eq!(projects["projects"][0]["slug"], "default");
    assert_eq!(projects["projects"][0]["id"], "default");
}

#[test]
fn file_replace_keeps_a_deployed_program_with_a_different_id() {
    let path = temp_db();
    let runtime = runtime(&path);
    let artifact = Artifact::sdk_first_example("deployed-hash");
    runtime
        .deploy(&json!({
            "name": "deployed",
            "artifact": {
                "hash": artifact.envelope.artifact_hash,
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "ts.subset.v1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": { "language": "typescript", "files": [] },
        }))
        .unwrap();
    runtime
        .replace_files(vec![file_program(
            "from-file",
            &Artifact::minimal_return("file-hash"),
        )])
        .unwrap();
    let ids: Vec<_> = runtime.list_programs().unwrap()["programs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|program| program["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["deployed".to_string(), "from-file".to_string()]);

    runtime
        .replace_files(vec![file_program(
            "deployed",
            &Artifact::minimal_return("file-wins"),
        )])
        .unwrap();
    let started = runtime.start(&json!({"programId": "deployed"})).unwrap();
    assert_eq!(started["execution"]["status"], "completed");
}

#[test]
fn nonempty_deployed_bundle_can_be_read_and_cannot_start() {
    let path = temp_db();
    let runtime = runtime(&path);
    let artifact = Artifact::minimal_return("bundle");
    runtime
        .deploy(&json!({
            "name": "effects",
            "artifact": {
                "hash": "bundle",
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": {
                "language": "typescript",
                "files": [{ "path": "worker.js", "contents": "export {}" }],
            },
        }))
        .unwrap();
    assert_eq!(
        runtime.get_program("effects").unwrap()["program"]["id"],
        "effects"
    );
    let error = runtime.start(&json!({"programId": "effects"})).unwrap_err();
    assert_eq!(
        error.message,
        "Local deploy execution does not support deployed effect bundles yet."
    );
}

#[test]
fn projects_and_deploys_survive_reopen() {
    let path = temp_db();
    let first = runtime(&path);
    let project = first.create_project(&json!({"name": "billing"})).unwrap();
    let artifact = wait_only();
    first
        .deploy(&json!({
            "name": "wait",
            "artifact": {
                "hash": "wait-only",
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "ts.subset.v1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": { "language": "typescript", "files": [] },
        }))
        .unwrap();
    drop(first);

    let second = runtime(&path);
    let slugs: Vec<_> = second.list_projects().unwrap()["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        slugs,
        vec![
            "default".to_string(),
            project["project"]["slug"].as_str().unwrap().to_string()
        ]
    );
    assert_eq!(second.list_programs().unwrap()["programs"][0]["id"], "wait");
    assert_eq!(
        second.list_versions("wait").unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let started = second.start(&json!({"programId": "wait"})).unwrap();
    assert_eq!(started["execution"]["status"], "waiting");
    assert_eq!(second.restored_waiting().unwrap(), 1);
}
