use std::path::PathBuf;
use std::sync::Arc;
use tcc_ir::{
    encode_artifact, Artifact, ConstValue, EngineFeature, Envelope, FuncId, Function,
    HostCapability, Instruction, LocalId, Program, ENGINE_FORMAT_VERSION, LANGUAGE_SEMANTICS_RUST,
};

use crate::{EventSink, FileProgram, LocalRuntime};

pub(super) fn quiet() -> EventSink {
    Arc::new(|_| {})
}

pub(super) fn temp_db() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trigora-local-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.db")
}

pub(super) fn file_program(id: &str, artifact: &Artifact) -> FileProgram {
    let artifact_json = encode_artifact(artifact).unwrap();
    FileProgram {
        id: id.to_string(),
        artifact_hash: artifact.envelope.artifact_hash.clone(),
        artifact_json,
        language: "typescript".to_string(),
        frontend_id: "typescript".to_string(),
        frontend_version: "0.0.0".to_string(),
        language_semantics_version: artifact.envelope.language_semantics_version.clone(),
        engine_format_version: ENGINE_FORMAT_VERSION as i64,
    }
}

pub(super) fn return_arg(hash: &str, semantics: &str) -> Artifact {
    let instructions = vec![
        Instruction::LoadLocal { local: LocalId(0) },
        Instruction::Return,
    ];
    Artifact {
        envelope: Envelope {
            language_semantics_version: semantics.to_string(),
            ..Envelope::typescript_v1(hash)
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 1,
                local_count: 1,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

pub(super) fn zero_arg_rust() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ];
    Artifact {
        envelope: Envelope {
            language_semantics_version: LANGUAGE_SEMANTICS_RUST.to_string(),
            ..Envelope::typescript_v1("rust-zero")
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

pub(super) fn wait_only() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::String("approved".to_string()),
        },
        Instruction::WaitForEvent,
        Instruction::Pop,
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ];
    let mut envelope = Envelope::typescript_v1("wait-only");
    envelope
        .required_engine_features
        .push(EngineFeature("durable.wait_for_event".to_string()));
    envelope
        .required_host_capabilities
        .push(HostCapability("host.event".to_string()));
    Artifact {
        envelope,
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

pub(super) fn wait_return_arg() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::String("approved".to_string()),
        },
        Instruction::WaitForEvent,
        Instruction::Pop,
        Instruction::LoadLocal { local: LocalId(0) },
        Instruction::Return,
    ];
    let mut envelope = Envelope::typescript_v1("wait-arg");
    envelope
        .required_engine_features
        .push(EngineFeature("durable.wait_for_event".to_string()));
    envelope
        .required_host_capabilities
        .push(HostCapability("host.event".to_string()));
    Artifact {
        envelope,
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 1,
                local_count: 1,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

pub(super) fn runtime(path: &PathBuf) -> LocalRuntime {
    LocalRuntime::open(path, None, quiet()).unwrap()
}
