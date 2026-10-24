use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tcc_ir::{
    Artifact, ConstValue, EngineFeature, Envelope, FuncId, Function, HostCapability, Instruction,
    Program, ENGINE_FORMAT_VERSION,
};
use tcc_state::{decode_continuation, encode_value, Value};

use crate::{Adapter, Authority, Config, Reply, Runtime};

fn scratch(label: &str) -> PathBuf {
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

fn artifact(name: &str, instructions: Vec<Instruction>) -> Artifact {
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

fn effects(keys: &[&str]) -> Vec<Instruction> {
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

fn echo(bytes: usize) -> Vec<Instruction> {
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

fn sleep_program() -> Vec<Instruction> {
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

fn invoke_program() -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            value: ConstValue::String("child".into()),
        },
        Instruction::Invoke { arg_count: 0 },
        Instruction::Return,
    ]
}

fn encoded(value: &Value) -> String {
    String::from_utf8(encode_value(value).expect("encode value")).expect("utf-8")
}

fn one() -> String {
    encoded(&Value::Number(1.0))
}

fn result_number(authority: &Authority, id: &str) -> f64 {
    let view = authority.view(id).expect("view");
    let bytes = view.continuation.expect("continuation");
    let continuation = decode_continuation(&bytes).expect("decode");
    match continuation.result {
        Some(Value::Number(number)) => number,
        other => panic!("expected a number result, got {other:?}"),
    }
}

fn keys_of(runtime: &Runtime) -> Vec<String> {
    runtime
        .dispatches()
        .into_iter()
        .map(|dispatch| dispatch.key)
        .collect()
}

fn wait_adapters(runtime: &Runtime, count: usize) {
    assert!(
        runtime.wait_until(
            |runtime| runtime.adapter_count() >= count,
            Duration::from_secs(2)
        ),
        "adapter did not register"
    );
}

fn serve(dir: &Path, hash: &str, keys: &[&str]) -> Adapter {
    let adapter = Adapter::connect(dir).expect("connect");
    adapter.register(hash, keys).expect("register");
    adapter.serve(|_| Reply::Value(one())).expect("serve");
    adapter
}

struct Host {
    dir: PathBuf,
    runtime: Runtime,
}

impl Host {
    fn open(_label: &str, authority: Arc<Authority>, program: Artifact, config: Config) -> Self {
        let dir = config.dir.clone();
        let runtime = Runtime::open(Arc::clone(&authority), program, config).expect("runtime");
        Self { dir, runtime }
    }

    fn start_with_adapter(&self, id: &str, hash: &str, keys: &[&str]) -> Adapter {
        let adapter = serve(&self.dir, hash, keys);
        wait_adapters(&self.runtime, 1);
        self.runtime.start(id).expect("start");
        adapter
    }
}

fn settings(dir: &Path, owner: &str) -> Config {
    Config::new(dir, owner)
}

#[test]
fn effect_commits_and_a_new_host_does_not_replay_it() {
    let authority = Arc::new(Authority::new());
    let program = artifact("effect", effects(&["work"]));
    let dir = scratch("effect");
    let host = Host::open(
        "effect",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "effect", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["work".to_string()]);
    assert_eq!(result_number(&authority, "exec"), 1.0);
    let journal = authority.view("exec").unwrap().journal;
    assert_eq!(journal["work"].status, "completed");
    drop(host);

    let dir = scratch("restore");
    let mut restored_config = settings(&dir, "B");
    restored_config.lease_ms = 1_000;
    authority.advance_clock(60_000).unwrap();
    let restored = Host::open("restore", Arc::clone(&authority), program, restored_config);
    restored.runtime.resume("exec").expect("resume");
    assert!(restored
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(result_number(&authority, "exec"), 1.0);
    let timings = restored.runtime.timings();
    eprintln!("warm restore {:?}", timings.restore);
    assert!(!timings.restore.is_empty());
}

#[test]
fn unconfirmed_progress_is_invisible_and_a_journaled_effect_is_not_redispatched() {
    let authority = Arc::new(Authority::new());
    let program = artifact("two", effects(&["first", "second"]));
    let dir = scratch("fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(0);
    let host = Host::open("fault", Arc::clone(&authority), program.clone(), config);
    let _adapter = host.start_with_adapter("exec", "two", &["first", "second"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let view = authority.view("exec").unwrap();
    assert_eq!(view.revision, 0);
    assert!(view.continuation.is_none());
    assert!(view.journal.is_empty());
    drop(host);

    let dir = scratch("again");
    let host = Host::open(
        "again",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "B"),
    );
    authority.advance_clock(60_000).unwrap();
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(
        keys_of(&host.runtime),
        vec!["first".to_string(), "second".to_string()]
    );
    drop(host);

    let dir = scratch("partial");
    let mut config = settings(&dir, "C");
    config.fault_after_commits = Some(1);
    authority.advance_clock(120_000).unwrap();
    let host = Host::open("partial", Arc::clone(&authority), program.clone(), config);
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("partial").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let view = authority.view("partial").unwrap();
    assert_eq!(view.revision, 1);
    assert!(view.journal.contains_key("first"));
    assert!(!view.journal.contains_key("second"));
    assert!(view.continuation.is_some());
    drop(host);

    let dir = scratch("partial2");
    authority.advance_clock(180_000).unwrap();
    let host = Host::open(
        "partial2",
        Arc::clone(&authority),
        program,
        settings(&dir, "D"),
    );
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("partial").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["second".to_string()]);
    assert!(authority
        .view("partial")
        .unwrap()
        .journal
        .contains_key("first"));
    assert!(authority
        .view("partial")
        .unwrap()
        .journal
        .contains_key("second"));
}

#[test]
fn sleep_resumes_through_a_claim_after_the_simulated_clock() {
    let authority = Arc::new(Authority::new());
    let program = artifact("sleep", sleep_program());
    let dir = scratch("sleep");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    let host = Host::open("sleep", Arc::clone(&authority), program.clone(), config);
    host.runtime.start("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5)));
    let view = authority.view("nap").unwrap();
    assert_eq!(view.timers.len(), 1);
    assert_eq!(view.timers[0].due_at, 5_000);
    assert!(host.runtime.dispatches().is_empty());
    drop(host);

    authority.advance_clock(5_000).unwrap();
    let dir = scratch("wake");
    let mut config = settings(&dir, "B");
    config.lease_ms = 1_000;
    let host = Host::open("wake", authority.clone(), program, config);
    host.runtime.resume("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(host.runtime.dispatches().is_empty());
    assert_eq!(result_number(&authority, "nap"), 1.0);
    assert!(authority.view("nap").unwrap().timers.is_empty());
}

#[test]
fn create_child_is_rejected() {
    let authority = Arc::new(Authority::new());
    let program = artifact("invoke", invoke_program());
    let dir = scratch("child");
    let host = Host::open("child", authority, program, settings(&dir, "A"));
    host.runtime.start("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let failures = host.runtime.failures();
    assert!(
        failures
            .iter()
            .any(|(_, message)| message.contains("create_child")),
        "{failures:?}"
    );
}

#[test]
fn stale_duplicate_and_disconnected_results_do_not_journal_a_success() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));

    let dir = scratch("stale");
    let host = Host::open(
        "stale",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::StaleThen(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("stale").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(authority.view("stale").unwrap().journal.len(), 1);
    drop(adapter);
    drop(host);

    let dir = scratch("dup");
    let host = Host::open(
        "dup",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::Duplicate(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("dup").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(authority.view("dup").unwrap().journal.len(), 1);
    drop(adapter);
    drop(host);

    let dir = scratch("drop");
    let host = Host::open("drop", Arc::clone(&authority), program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::Disconnect).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("drop").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.abandoned(), 0);
    let view = authority.view("drop").unwrap();
    assert!(view.journal.is_empty());
    assert!(view.continuation.is_none());
    assert_eq!(view.revision, 0);
}

#[test]
fn timeout_retries_with_a_new_attempt_and_ignores_the_late_result() {
    let authority = Arc::new(Authority::new());
    let program = artifact("slow", effects(&["work"]));
    let dir = scratch("timeout");
    let mut config = settings(&dir, "A");
    config.effect_timeout = Duration::from_millis(50);
    config.max_effect_attempts = 2;
    let host = Host::open("timeout", authority.clone(), program, config);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("slow", &["work"]).unwrap();
    adapter
        .serve(|request| {
            if request.attempt_id.ends_with(":1") {
                Reply::Delay(Duration::from_millis(200), one())
            } else {
                Reply::Value(one())
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("slow").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let attempts: Vec<_> = host
        .runtime
        .dispatches()
        .into_iter()
        .map(|dispatch| dispatch.attempt_id)
        .collect();
    assert_eq!(attempts, vec!["slow:1".to_string(), "slow:2".to_string()]);
    assert_eq!(authority.view("slow").unwrap().journal.len(), 1);
    assert_eq!(
        authority.view("slow").unwrap().journal["work"].status,
        "completed"
    );
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(authority.view("slow").unwrap().journal.len(), 1);
    drop(adapter);
}

#[test]
fn two_adapters_for_one_hash_and_a_late_result_after_a_new_owner() {
    let authority = Arc::new(Authority::new());
    let program = artifact("shared", effects(&["work"]));
    let dir = scratch("shared");
    let host = Host::open(
        "shared",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let first_hits = Arc::new(AtomicUsize::new(0));
    let second_hits = Arc::new(AtomicUsize::new(0));
    let first = Adapter::connect(&dir).unwrap();
    first.register("shared", &["work"]).unwrap();
    let hits = Arc::clone(&first_hits);
    first
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    let second = Adapter::connect(&dir).unwrap();
    second.register("shared", &["work"]).unwrap();
    let hits = Arc::clone(&second_hits);
    second
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 2);
    host.runtime.start("shared").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(first_hits.load(Ordering::SeqCst), 1);
    assert_eq!(second_hits.load(Ordering::SeqCst), 0);
    second.push_result("shared", "shared:9", &one()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(authority.view("shared").unwrap().journal.len(), 1);
    drop(first);
    drop(second);
    drop(host);

    let dir = scratch("owner");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    config.effect_timeout = Duration::from_secs(5);
    let host = Host::open("owner", Arc::clone(&authority), program, config);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let parked = Arc::clone(&gate);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("shared", &["work"]).unwrap();
    adapter
        .serve(move |_| {
            let (lock, cv) = &*parked;
            let mut open = lock.lock().expect("gate");
            while !*open {
                open = cv.wait(open).expect("gate wait");
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("owned").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.dispatches().len() == 1,
        Duration::from_secs(5)
    ));
    let attempt = host.runtime.dispatches()[0].attempt_id.clone();
    authority.advance_clock(1_000).unwrap();
    authority.claim("owned", "B", 1_000).unwrap();
    adapter.disconnect();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.abandoned(), 0);
    *gate.0.lock().expect("gate") = true;
    gate.1.notify_all();
    let late = Adapter::connect(&dir).unwrap();
    late.register("shared", &["work"]).unwrap();
    wait_adapters(&host.runtime, 1);
    late.push_result("owned", &attempt, &one()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let view = authority.view("owned").unwrap();
    assert!(view.journal.is_empty());
    assert_eq!(view.revision, 0);
    assert_eq!(view.owner.as_deref(), Some("B"));
    assert_eq!(host.runtime.completed(), 0);
    drop(late);
}

#[test]
fn disconnecting_every_adapter_leaves_the_execution_recoverable() {
    let authority = Arc::new(Authority::new());
    let program = artifact("recover", effects(&["work"]));
    let dir = scratch("recover");
    let host = Host::open(
        "recover",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("recover", &["work"]).unwrap();
    adapter.serve(|_| Reply::Disconnect).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.abandoned(), 0);
    assert!(authority.view("exec").unwrap().journal.is_empty());
    drop(adapter);
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("recover2");
    let host = Host::open(
        "recover2",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    let _adapter = serve(&host.dir, "recover", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(result_number(&authority, "exec"), 1.0);
}

#[test]
fn blocked_callbacks_do_not_stall_the_other_executions() {
    let authority = Arc::new(Authority::new());
    let program = artifact("mix", effects(&["work"]));
    let dir = scratch("mix");
    let mut config = settings(&dir, "A");
    config.workers = 4;
    config.cap = 20;
    config.effect_timeout = Duration::from_secs(10);
    let host = Host::open("mix", authority, program, config);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let gate = Arc::clone(&release);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("mix", &["work"]).unwrap();
    adapter
        .serve(move |request| {
            if request.execution_id.starts_with("block") {
                let (lock, cv) = &*gate;
                let mut open = lock.lock().expect("gate");
                while !*open {
                    open = cv.wait(open).expect("gate wait");
                }
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    for index in 0..10 {
        host.runtime.start(&format!("block-{index}")).unwrap();
    }
    assert!(host.runtime.wait_until(
        |runtime| runtime.dispatches().len() == 10,
        Duration::from_secs(5)
    ));
    for index in 0..90 {
        host.runtime.start(&format!("fast-{index}")).unwrap();
    }
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 90, Duration::from_secs(15)),
        "completed {} inflight {} abandoned {}",
        host.runtime.completed(),
        host.runtime.inflight(),
        host.runtime.abandoned()
    );
    assert!(host.runtime.backlogged());
    assert!(host.runtime.max_inflight() <= 20);
    assert_eq!(host.runtime.completed(), 90);
    *release.0.lock().expect("gate") = true;
    release.1.notify_all();
    assert!(host.runtime.wait_until(
        |runtime| runtime.completed() == 100,
        Duration::from_secs(15)
    ));
    drop(adapter);
}

#[test]
fn payload_and_commit_timings_are_recorded() {
    let authority = Arc::new(Authority::new());
    for bytes in [4 * 1024, 64 * 1024, 1024 * 1024] {
        let name = format!("echo-{bytes}");
        let program = artifact(&name, echo(bytes));
        let dir = scratch("echo");
        let host = Host::open("echo", Arc::clone(&authority), program, settings(&dir, "A"));
        let adapter = Adapter::connect(&dir).unwrap();
        adapter.register(&name, &["echo"]).unwrap();
        adapter
            .serve(|request| Reply::Value(request.input))
            .unwrap();
        wait_adapters(&host.runtime, 1);
        let id = format!("echo-{bytes}");
        host.runtime.start(&id).unwrap();
        assert!(
            host.runtime
                .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(20)),
            "{bytes} abandoned {} failures {:?}",
            host.runtime.abandoned(),
            host.runtime.failures()
        );
        let timings = host.runtime.timings();
        eprintln!(
            "payload {bytes} socket {:?} commit {:?}",
            timings.socket, timings.commit
        );
        assert!(!timings.socket.is_empty());
        assert!(!timings.commit.is_empty());
        assert_eq!(result_number(&authority, &id), 1.0);
        drop(adapter);
    }
}

#[test]
fn resumed_execution_reads_only_the_claimed_continuation() {
    let authority = Arc::new(Authority::new());
    let program = artifact("claim", effects(&["work"]));
    let dir = scratch("claim");
    let host = Host::open(
        "claim",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "claim", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let committed = authority.view("exec").unwrap().continuation.unwrap();
    drop(host);
    authority.advance_clock(60_000).unwrap();
    let dir = scratch("claim2");
    let host = Host::open(
        "claim2",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let again = authority.view("exec").unwrap().continuation.unwrap();
    assert_eq!(committed, again);
    assert!(host.runtime.dispatches().is_empty());
}

#[test]
fn started_accepts_a_fresh_id_and_rejects_a_reused_one() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));
    let dir = scratch("start");
    let host = Host::open("start", authority, program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.start("exec").unwrap();
    let err = host.runtime.start("exec").unwrap_err();
    assert_eq!(err.to_string(), "execution id is already in use");
    adapter.serve(|_| Reply::Value(one())).unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let err = host.runtime.start("exec").unwrap_err();
    assert_eq!(err.to_string(), "execution id is already in use");
}

#[test]
fn a_missing_handler_key_is_not_dispatched() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));
    let dir = scratch("key");
    let host = Host::open("key", authority, program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["other"]).unwrap();
    adapter.serve(|_| Reply::Value(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert!(host.runtime.dispatches().is_empty());
    assert_eq!(host.runtime.completed(), 0);
    assert!(host
        .runtime
        .failures()
        .iter()
        .any(|(_, message)| message.contains("no adapter registered for effect `work`")));
}

#[test]
fn registering_an_adapter_does_not_resume_sleep() {
    let authority = Arc::new(Authority::new());
    let program = artifact("sleep", sleep_program());
    let dir = scratch("sleep-reg");
    let host = Host::open("sleep-reg", authority, program, settings(&dir, "A"));
    host.runtime.start("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5)));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("sleep", &["work"]).unwrap();
    wait_adapters(&host.runtime, 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(host.runtime.suspended(), 1);
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.waiting_for_adapter(), 0);
    drop(adapter);
}

#[test]
fn reconnecting_to_the_same_host_retries_only_the_outstanding_effect() {
    let authority = Arc::new(Authority::new());
    let program = artifact("two", effects(&["first", "second"]));
    let dir = scratch("live");
    let host = Host::open("live", Arc::clone(&authority), program, settings(&dir, "A"));
    let first_calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&first_calls);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("two", &["first", "second"]).unwrap();
    adapter
        .serve(move |request| {
            if request.key == "first" {
                seen.fetch_add(1, Ordering::SeqCst);
                Reply::Value(one())
            } else {
                Reply::Disconnect
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert!(authority
        .view("exec")
        .unwrap()
        .journal
        .contains_key("first"));
    let stale = host.runtime.dispatches().last().unwrap().attempt_id.clone();
    drop(adapter);

    let replacement_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&replacement_calls);
    let replacement = Adapter::connect(&dir).unwrap();
    replacement.register("two", &["first", "second"]).unwrap();
    replacement.push_result("exec", &stale, &one()).unwrap();
    replacement
        .serve(move |request| {
            seen.lock().expect("calls").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(
        replacement_calls.lock().expect("calls").as_slice(),
        ["second".to_string()]
    );
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    let journal = authority.view("exec").unwrap().journal;
    assert!(journal.contains_key("first"));
    assert!(journal.contains_key("second"));
}

#[test]
fn a_node_handler_is_not_replayed_after_the_host_is_replaced() {
    let authority = Arc::new(Authority::new());
    let program = artifact("effect", effects(&["work"]));
    let dir = scratch("node-host");
    let host = Host::open(
        "node-host",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let peer = NodePeer::spawn(&dir, "effect", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["work".to_string()]);
    assert!(authority.view("exec").unwrap().journal.contains_key("work"));
    drop(peer);
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("node-restore");
    let restored = Host::open(
        "node-restore",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    restored.runtime.resume("exec").unwrap();
    assert!(restored
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(restored.runtime.dispatches().is_empty());
    let peer = NodePeer::spawn(&dir, "effect", &["work"]);
    wait_adapters(&restored.runtime, 1);
    std::thread::sleep(Duration::from_millis(100));
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(restored.runtime.completed(), 1);
    assert_eq!(result_number(&authority, "exec"), 1.0);
    drop(peer);
}

struct NodePeer {
    child: std::process::Child,
}

impl NodePeer {
    fn spawn(dir: &Path, hash: &str, keys: &[&str]) -> Self {
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

#[test]
fn remote_commit_survives_the_host_and_is_not_replayed() {
    let server = crate::remote::serve();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let program = artifact("remote", effects(&["work"]));
    let dir = scratch("remote");
    let host = Host::open(
        "remote",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "remote", &["work"]);
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(15)),
        "abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    let committed = authority
        .view("exec")
        .expect("view")
        .continuation
        .expect("continuation");
    let report = authority.remote_report().expect("remote report");
    eprintln!(
        "remote continuation {} journal {} objects {} validation_us {} compactions {}",
        report.continuation_bytes,
        report.journal_bytes,
        report.objects,
        report.validation_us,
        report.compactions
    );
    assert!(report.continuation_bytes > 0);
    assert!(report.objects >= 1);
    drop(host);
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("remote-restored");
    let host = Host::open(
        "remote-restored",
        Arc::clone(&authority),
        program,
        settings(&dir, "A"),
    );
    host.runtime.resume("exec").expect("resume");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(15)),
        "restored abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    assert!(host.runtime.dispatches().is_empty());
    let again = authority
        .view("exec")
        .expect("view")
        .continuation
        .expect("continuation");
    assert_eq!(committed, again);
}

#[test]
fn a_held_commit_does_not_stall_another_execution_and_a_late_ack_is_dropped() {
    let server = crate::remote::serve();
    let probe = server.client();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("held");
    let mut config = settings(&dir, "A");
    config.lease_ms = 400;
    let host = Host::open(
        "held",
        Arc::clone(&authority),
        artifact("pair", effects(&["first", "second"])),
        config,
    );
    probe.test_hook("/v1/test/hold", "slow").expect("hold");
    let _adapter = host.start_with_adapter("fast", "pair", &["first", "second"]);
    host.runtime.start("slow").expect("start slow");
    assert!(
        host.runtime.wait_until(
            |runtime| { runtime.completed() == 1 && probe.test_renewals("slow").unwrap_or(0) >= 1 },
            Duration::from_secs(8),
        ),
        "fast completed {} slow renewals {:?} abandoned {} failures {:?}",
        host.runtime.completed(),
        probe.test_renewals("slow"),
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    probe
        .test_hook("/v1/test/fail-renew", "slow")
        .expect("fail renew");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() >= 1, Duration::from_secs(5)),
        "slow was not dropped"
    );
    probe
        .test_hook("/v1/test/release", "slow")
        .expect("release");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(host.runtime.completed(), 1);
    assert_eq!(
        keys_of(&host.runtime)
            .iter()
            .filter(|key| key.as_str() == "second")
            .count(),
        1
    );
}

#[test]
fn a_late_callback_does_not_advance_after_ownership_is_lost() {
    let server = crate::remote::serve();
    let probe = server.client();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("late-callback");
    let mut config = settings(&dir, "A");
    config.lease_ms = 400;
    let host = Host::open(
        "late-callback",
        Arc::clone(&authority),
        artifact("late", effects(&["work"])),
        config,
    );
    let (gate_tx, gate_rx) = std::sync::mpsc::channel();
    let gate_rx = Arc::new(Mutex::new(gate_rx));
    let adapter = Adapter::connect(&dir).expect("connect");
    adapter.register("late", &["work"]).expect("register");
    let gate = Arc::clone(&gate_rx);
    adapter
        .serve(move |_| {
            let _ = gate.lock().expect("gate").recv();
            Reply::Value(one())
        })
        .expect("serve");
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").expect("start");
    assert!(
        host.runtime.wait_until(
            |_| probe.test_renewals("exec").unwrap_or(0) >= 1,
            Duration::from_secs(5),
        ),
        "renewal did not run"
    );
    probe
        .test_hook("/v1/test/fail-renew", "exec")
        .expect("fail renew");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() >= 1, Duration::from_secs(5)),
        "ownership loss did not stop the execution"
    );
    let _ = gate_tx.send(());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(host.runtime.completed(), 0);
    let view = authority.view("exec").expect("view");
    assert_eq!(view.revision, 0);
    assert!(view.journal.is_empty());
}
