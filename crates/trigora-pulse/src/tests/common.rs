use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tcc_ir::{
    Artifact, ConstValue, EngineFeature, Envelope, FuncId, Function, HostCapability, Instruction,
    Program, ENGINE_FORMAT_VERSION,
};
use tcc_state::{decode_continuation, encode_value, Value};

use crate::{Adapter, Authority, Config, Reply, Runtime};

pub(super) fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root.pop();
    root.push("target");
    root.push("pulse");
    let path = root.join(format!("p{}-{n}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

pub(super) fn artifact(name: &str, instructions: Vec<Instruction>) -> Artifact {
    let spans = vec![None; instructions.len()];
    Artifact {
        envelope: Envelope {
            artifact_hash: name.into(),
            frontend_id: "typescript".into(),
            frontend_version: "0.0.0".into(),
            language_semantics_version: "ts.subset.v1".into(),
            engine_format_version: ENGINE_FORMAT_VERSION,
            required_engine_features: FeatureSet::current(),
            required_host_capabilities: HostCapability::known()
                .iter()
                .map(|id| HostCapability((*id).to_string()))
                .collect(),
            runtime_modules: Vec::new(),
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".into(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                instructions,
                spans,
            }],
        },
    }
}

struct FeatureSet;

impl FeatureSet {
    fn current() -> Vec<EngineFeature> {
        tcc_ir::FeatureSet::current_engine()
    }
}

pub(super) fn effects(keys: &[&str]) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    for key in keys {
        instructions.push(Instruction::LoadConst {
            value: ConstValue::String((*key).to_string()),
        });
        instructions.push(Instruction::Effect { has_input: false });
        instructions.push(Instruction::Pop);
    }
    instructions.push(Instruction::LoadConst {
        value: ConstValue::Number(1.0),
    });
    instructions.push(Instruction::Return);
    instructions
}

pub(super) fn echo(bytes: usize) -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            value: ConstValue::String("x".repeat(bytes)),
        },
        Instruction::LoadConst {
            value: ConstValue::String("echo".into()),
        },
        Instruction::Effect { has_input: true },
        Instruction::Pop,
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ]
}

pub(super) fn sleep_program() -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            value: ConstValue::Number(5_000.0),
        },
        Instruction::Sleep,
        Instruction::Pop,
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ]
}

pub(super) fn invoke_program() -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            value: ConstValue::String("child".into()),
        },
        Instruction::Invoke { arg_count: 0 },
        Instruction::Return,
    ]
}

pub(super) fn encoded(value: &Value) -> String {
    String::from_utf8(encode_value(value).expect("encode value")).expect("utf-8")
}

pub(super) fn one() -> String {
    encoded(&Value::Number(1.0))
}

pub(super) fn result_number(authority: &Authority, id: &str) -> f64 {
    let view = authority.view(id).expect("view");
    let bytes = view.continuation.expect("continuation");
    let continuation = decode_continuation(&bytes).expect("decode");
    match continuation.result {
        Some(Value::Number(number)) => number,
        other => panic!("expected a number result, got {other:?}"),
    }
}

pub(super) fn keys_of(runtime: &Runtime) -> Vec<String> {
    runtime
        .dispatches()
        .into_iter()
        .map(|dispatch| dispatch.key)
        .collect()
}

pub(super) fn wait_adapters(runtime: &Runtime, count: usize) {
    assert!(
        runtime.wait_until(
            |runtime| runtime.adapter_count() >= count,
            Duration::from_secs(2)
        ),
        "adapter did not register"
    );
}

pub(super) fn serve(dir: &Path, hash: &str, keys: &[&str]) -> Adapter {
    let adapter = Adapter::connect(dir).expect("connect");
    adapter.register(hash, keys).expect("register");
    adapter.serve(|_| Reply::Value(one())).expect("serve");
    adapter
}

pub(super) struct Host {
    pub(super) dir: PathBuf,
    pub(super) runtime: Runtime,
}

impl Host {
    pub(super) fn open(
        _label: &str,
        authority: Arc<Authority>,
        program: Artifact,
        config: Config,
    ) -> Self {
        let dir = config.dir.clone();
        let runtime = Runtime::open(Arc::clone(&authority), program, config).expect("runtime");
        Self { dir, runtime }
    }

    pub(super) fn start_with_adapter(&self, id: &str, hash: &str, keys: &[&str]) -> Adapter {
        let adapter = serve(&self.dir, hash, keys);
        wait_adapters(&self.runtime, 1);
        self.runtime.start(id).expect("start");
        adapter
    }
}

pub(super) fn settings(dir: &Path, owner: &str) -> Config {
    Config::new(dir, owner)
}

pub(super) struct NodePeer {
    child: std::process::Child,
}

impl NodePeer {
    pub(super) fn spawn(dir: &Path, hash: &str, keys: &[&str]) -> Self {
        let mut script = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        script.pop();
        script.pop();
        script.push("packages");
        script.push("pulse-node");
        script.push("src");
        script.push("peer.mjs");
        let mut command = std::process::Command::new("node");
        command.arg(&script).arg(dir).arg(hash);
        for key in keys {
            command.arg(key);
        }
        let child = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn node adapter");
        Self { child }
    }
}

impl Drop for NodePeer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
