use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use tcc_ir::{
    encode_artifact, Artifact, ConstValue, Envelope, FuncId, Function, Instruction, Program,
};

fn program(instructions: Vec<Instruction>, locals: u32) -> (String, String) {
    let artifact = Artifact {
        envelope: Envelope::typescript_v1("bench-hash"),
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 0,
                local_count: locals,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    };
    let json = encode_artifact(&artifact).unwrap();
    (json, artifact.envelope.artifact_hash)
}

fn request(json: String, hash: String, effects: EffectSource, faults: Option<Faults>) -> Request {
    let has_effects = !matches!(effects, EffectSource::Absent);
    Request {
        name: "sample".to_string(),
        artifact_json: json,
        artifact_hash: hash,
        input: None,
        has_effects,
        effects,
        events: Vec::new(),
        faults,
    }
}

#[test]
fn bench_boundary_count_matches_the_trace() {
    let (json, hash) = program(
        vec![
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
        0,
    );
    let report = run(request(json, hash, EffectSource::Absent, None)).unwrap();
    assert_eq!(
        report.baseline.durable_boundaries as usize,
        report.checkpoints.len()
    );
    assert!(report.baseline.durable_boundaries >= 1);
    assert!(report.fault_injection.is_none());
    assert!(report.baseline.continuation_restore_ms.p50.is_some());
}

#[test]
fn verify_sample_recovers_with_zero_mismatches() {
    let (json, hash) = program(
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("approval".to_string()),
            },
            Instruction::WaitForEvent,
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
        0,
    );
    let report = run(request(
        json,
        hash,
        EffectSource::Absent,
        Some(Faults::Sample),
    ))
    .unwrap();
    let faults = report.fault_injection.unwrap();
    assert!(faults.fault_points_tested >= 1);
    assert_eq!(faults.result_mismatches, 0);
    assert_eq!(faults.failed_recoveries, 0);
    assert_eq!(faults.duplicate_effects, 0);
    assert_eq!(faults.successful_recoveries, faults.fault_points_tested);
}

#[test]
fn effects_without_a_source_are_refused() {
    let (json, hash) = program(
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("charge".to_string()),
            },
            Instruction::Effect { has_input: false },
            Instruction::Return,
        ],
        0,
    );
    let error = run(Request {
        has_effects: true,
        effects: EffectSource::Absent,
        ..request(json, hash, EffectSource::Absent, None)
    })
    .unwrap_err();
    assert!(error.title.contains("Effects"));
}

#[test]
fn live_effects_record_once_and_resume_hits_the_journal() {
    let (json, hash) = program(
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("charge".to_string()),
            },
            Instruction::Effect { has_input: false },
            Instruction::Return,
        ],
        0,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let seen = calls.clone();
    let report = run(request(
        json,
        hash,
        EffectSource::Live(Box::new(move |_key, _input| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Number(7.0))
        })),
        Some(Faults::All),
    ))
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let faults = report.fault_injection.unwrap();
    assert_eq!(faults.duplicate_effects, 0);
    assert_eq!(faults.missing_effects, 0);
    assert_eq!(faults.result_mismatches, 0);
    assert!(
        faults
            .cases
            .iter()
            .any(|case| case.resume_provider_invocations == 0),
        "recovery after a committed effect must not call the provider again: {:?}",
        faults
            .cases
            .iter()
            .map(|case| (
                case.boundary,
                case.resume_provider_invocations,
                case.journal_hits
            ))
            .collect::<Vec<_>>()
    );
}
