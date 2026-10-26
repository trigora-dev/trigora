use serde_json::{json, Value as Json};

use super::common::*;

#[test]
fn fresh_start_binds_normalized_input_and_resume_does_not() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program(
            "echo",
            &return_arg("echo", "ts.subset.v1"),
        )])
        .unwrap();
    let from_array = runtime
        .start(&json!({"programId": "echo", "input": [7]}))
        .unwrap();
    assert_eq!(from_array["execution"]["result"], 7);
    assert_eq!(from_array["execution"]["input"], json!([7]));

    let from_value = runtime
        .start(&json!({"programId": "echo", "input": 9}))
        .unwrap();
    assert_eq!(from_value["execution"]["result"], 9);

    let omitted = runtime.start(&json!({"programId": "echo"})).unwrap();
    assert_eq!(omitted["execution"]["input"], Json::Null);
    assert_eq!(omitted["execution"]["result"], Json::Null);

    runtime
        .replace_files(vec![
            file_program("echo", &return_arg("echo", "ts.subset.v1")),
            file_program("wait", &wait_return_arg()),
        ])
        .unwrap();
    let waiting = runtime
        .start(&json!({"programId": "wait", "input": [7]}))
        .unwrap();
    assert_eq!(waiting["execution"]["status"], "waiting");
    let resumed = runtime
        .send(
            waiting["execution"]["id"].as_str().unwrap(),
            &json!({"name": "approved", "payload": "replaced"}),
        )
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(resumed["execution"]["result"], 7);
}

#[test]
fn rust_rejects_an_object_passed_to_a_zero_arg_program() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program("main", &zero_arg_rust())])
        .unwrap();
    let error = runtime
        .start(&json!({"programId": "main", "input": {}}))
        .unwrap_err();
    assert!(
        error.message.contains("positional arguments"),
        "{}",
        error.message
    );
}

#[test]
fn waiting_execution_survives_reopen_and_then_resumes() {
    let path = temp_db();
    let first = runtime(&path);
    first
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    let started = first.start(&json!({"programId": "wait"})).unwrap();
    let id = started["execution"]["id"].as_str().unwrap().to_string();
    drop(first);

    let second = runtime(&path);
    second
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    assert_eq!(second.restored_waiting().unwrap(), 1);
    let resumed = second
        .send(&id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(resumed["execution"]["result"], 1);
}

#[test]
fn rejects_a_mismatched_event_and_cancels_a_wait() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    let started = runtime.start(&json!({"programId": "wait"})).unwrap();
    let id = started["execution"]["id"].as_str().unwrap();
    let mismatch = runtime
        .send(id, &json!({"name": "rejected", "payload": "ok"}))
        .unwrap_err();
    assert_eq!(mismatch.code, "event_mismatch");

    let missing = runtime.start(&json!({"programId": "missing"})).unwrap_err();
    assert_eq!(missing.code, "program_not_found");

    let cancelled = runtime.cancel(id).unwrap();
    assert_eq!(cancelled["execution"]["status"], "cancelled");
    let after = runtime
        .send(id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap_err();
    assert_eq!(after.code, "execution_not_waiting");
    let listed = runtime.list_executions().unwrap();
    assert_eq!(listed["executions"][0]["id"], id);
}
