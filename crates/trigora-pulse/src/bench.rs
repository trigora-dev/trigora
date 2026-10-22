use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde_json::{json, Value as Json};
use tcc_ir::{
    Artifact, ConstValue, EngineFeature, Envelope, FuncId, Function, HostCapability, Instruction,
    Program, ENGINE_FORMAT_VERSION,
};
use tcc_state::{encode_continuation, Continuation, Value};
use trigora_pulse::{Adapter, Authority, Config, JournalRow, Reply, Runtime, Span};

pub fn run() -> std::process::ExitCode {
    let cpu = cpu_model();
    let cpus = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    println!("pulse stage bench");
    println!(
        "profile release  arch {}  cpus {cpus}  cpu {cpu}",
        std::env::consts::ARCH
    );
    println!("isolation dedicated process; sample clocks are not compared across peers");
    println!("peer rust-echo  frames 1 effect + 1 result  value echoed unchanged");

    let throughput = match throughput_rust() {
        Ok(row) => row,
        Err(err) => {
            eprintln!("throughput: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    println!(
        "throughput {} small effects in {:.3} ms  {:.1}/s  rss {} bytes",
        throughput.0, throughput.1, throughput.2, throughput.3
    );

    let mut sizes = Vec::new();
    for (bytes, warmup, samples) in [
        (4 * 1024, 20, 200),
        (64 * 1024, 20, 200),
        (1024 * 1024, 10, 100),
    ] {
        match measure_rust_peer(bytes, warmup, samples) {
            Ok(spans) => {
                print_spans("rust-peer", bytes, &spans);
                sizes.push(size_json(bytes, &spans));
            }
            Err(err) => {
                eprintln!("rust peer {bytes}: {err}");
                return std::process::ExitCode::from(1);
            }
        }
    }

    let checkpoint = checkpoint_report();
    for row in &checkpoint {
        println!("{row}");
    }

    let report = json!({
        "profile": "release",
        "arch": std::env::consts::ARCH,
        "cpus": cpus,
        "cpu": cpu,
        "peer": "rust-echo",
        "sizes": sizes,
        "throughput": {
            "effects": throughput.0,
            "elapsedMs": throughput.1,
            "perSecond": throughput.2,
            "rssBytes": throughput.3,
        },
        "checkpoint": checkpoint,
    });
    let path = bench_root().join("bench-rust.json");
    fs::write(&path, serde_json::to_string_pretty(&report).expect("json")).expect("write report");
    println!("wrote {}", path.display());
    std::process::ExitCode::SUCCESS
}

fn measure_rust_peer(bytes: usize, warmup: usize, samples: usize) -> Result<Vec<Span>, String> {
    let name = format!("echo-{bytes}");
    let dir = scratch(&format!("rust-{bytes}"));
    let mut config = Config::new(&dir, "bench");
    config.effect_timeout = Duration::from_secs(30);
    let runtime = Runtime::open(
        Arc::new(Authority::new()),
        artifact(&name, echo(bytes)),
        config,
    )
    .map_err(|err| err.to_string())?;
    let adapter = Adapter::connect(runtime.dir()).map_err(|err| err.to_string())?;
    adapter
        .register(&name, &["echo"])
        .map_err(|err| err.to_string())?;
    adapter
        .serve(|request| Reply::Value(request.input))
        .map_err(|err| err.to_string())?;
    if !runtime.wait_until(
        |runtime| runtime.adapter_count() >= 1,
        Duration::from_secs(2),
    ) {
        return Err("adapter did not register".into());
    }
    run_serial(&runtime, "w", warmup)?;
    runtime.clear_timings();
    run_serial(&runtime, "m", samples)?;
    Ok(runtime.timings().spans)
}

fn run_n(runtime: &Runtime, prefix: &str, count: usize) -> Result<(), String> {
    for index in 0..count {
        runtime
            .start(&format!("{prefix}-{index}"))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// One execution at a time, so a latency sample is not the wait behind the other samples.
fn run_serial(runtime: &Runtime, prefix: &str, count: usize) -> Result<(), String> {
    for index in 0..count {
        let before = runtime.completed();
        runtime
            .start(&format!("{prefix}-{index}"))
            .map_err(|err| err.to_string())?;
        if !runtime.wait_until(
            |runtime| runtime.completed() == before + 1,
            Duration::from_secs(30),
        ) {
            return Err(format!(
                "completed {} failures {:?}",
                runtime.completed(),
                runtime.failures()
            ));
        }
    }
    Ok(())
}

fn throughput_rust() -> Result<(usize, f64, f64, u64), String> {
    let dir = scratch("throughput");
    let mut config = Config::new(&dir, "bench");
    config.effect_timeout = Duration::from_secs(30);
    let runtime = Runtime::open(
        Arc::new(Authority::new()),
        artifact("small", echo(32)),
        config,
    )
    .map_err(|err| err.to_string())?;
    let adapter = Adapter::connect(runtime.dir()).map_err(|err| err.to_string())?;
    adapter
        .register("small", &["echo"])
        .map_err(|err| err.to_string())?;
    adapter
        .serve(|request| Reply::Value(request.input))
        .map_err(|err| err.to_string())?;
    if !runtime.wait_until(
        |runtime| runtime.adapter_count() >= 1,
        Duration::from_secs(2),
    ) {
        return Err("adapter did not register".into());
    }
    let count = 64;
    let started = std::time::Instant::now();
    run_n(&runtime, "t", count)?;
    if !runtime.wait_until(
        |runtime| runtime.completed() == count,
        Duration::from_secs(30),
    ) {
        return Err(format!("throughput completed {}", runtime.completed()));
    }
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let per_second = count as f64 / (elapsed / 1000.0);
    Ok((count, elapsed, per_second, current_rss()))
}

fn checkpoint_report() -> Vec<String> {
    let cases: [(&str, usize, usize, usize); 4] = [
        ("small-live short-journal", 0, 1, 32),
        ("large-live short-journal", 1024 * 1024, 1, 32),
        ("small-live large-journal", 0, 64, 16 * 1024),
        ("large-live large-journal", 1024 * 1024, 64, 16 * 1024),
    ];
    let mut lines = Vec::new();
    lines.push(
        "checkpoint  continuation is live state; each commit still carries the full journal"
            .to_string(),
    );
    for (label, live, rows, row_bytes) in cases {
        let mut previous = BTreeMap::new();
        for index in 0..rows.saturating_sub(1) {
            previous.insert(format!("e{index}"), journal_row(row_bytes));
        }
        let mut current = previous.clone();
        if rows > 0 {
            current.insert(format!("e{}", rows - 1), journal_row(row_bytes));
        }
        let previous_bytes = journal_wire(&previous).len();
        let journal_bytes = journal_wire(&current).len();
        let continuation = continuation_with_live(live);
        let mut samples = Vec::new();
        let mut encoded = Vec::new();
        for _ in 0..50 {
            let started = std::time::Instant::now();
            encoded = encode_continuation(&continuation).expect("encode continuation");
            samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        let compressed_continuation = deflate(&encoded);
        let compressed_journal = deflate(journal_wire(&current).as_bytes());
        lines.push(format!(
            "{label}  continuation {} bytes (deflate {}) p50 {:.1} µs  journal {} bytes of which {} were already committed (deflate {})",
            encoded.len(),
            compressed_continuation.len(),
            percentile(&mut samples, 50.0),
            journal_bytes,
            previous_bytes,
            compressed_journal.len(),
        ));
    }
    lines
}

fn print_spans(peer: &str, bytes: usize, spans: &[Span]) {
    let label = if bytes >= 1024 * 1024 {
        "1 MB".to_string()
    } else {
        format!("{} KB", bytes / 1024)
    };
    println!("{peer} {label} n={}", spans.len());
    for (name, values) in [
        ("drive", micros(spans, |span| span.drive)),
        ("encodeInput", micros(spans, |span| span.encode_input)),
        ("schedule", micros(spans, |span| span.schedule)),
        ("socketWrite", micros(spans, |span| span.socket_write)),
        ("socketRead", micros(spans, |span| span.socket_read)),
        ("roundTrip", micros(spans, |span| span.round_trip)),
        ("decodeResult", micros(spans, |span| span.decode_result)),
        ("journal", micros(spans, |span| span.journal)),
        ("continuation", micros(spans, |span| span.continuation)),
        ("commit", micros(spans, |span| span.commit)),
    ] {
        println!(
            "  {name:<14} p50 {:8.1} µs  p95 {:8.1} µs  p99 {:8.1} µs",
            percentile(&mut values.clone(), 50.0),
            percentile(&mut values.clone(), 95.0),
            percentile(&mut values.clone(), 99.0),
        );
    }
}

fn size_json(bytes: usize, spans: &[Span]) -> Json {
    let mut row = json!({ "bytes": bytes, "samples": spans.len() });
    let object = row.as_object_mut().expect("object");
    for (name, mut values) in [
        ("driveUs", micros(spans, |span| span.drive)),
        ("encodeInputUs", micros(spans, |span| span.encode_input)),
        ("scheduleUs", micros(spans, |span| span.schedule)),
        ("socketWriteUs", micros(spans, |span| span.socket_write)),
        ("socketReadUs", micros(spans, |span| span.socket_read)),
        ("roundTripUs", micros(spans, |span| span.round_trip)),
        ("decodeResultUs", micros(spans, |span| span.decode_result)),
        ("journalUs", micros(spans, |span| span.journal)),
        ("continuationUs", micros(spans, |span| span.continuation)),
        ("commitUs", micros(spans, |span| span.commit)),
    ] {
        let p50 = percentile(&mut values, 50.0);
        let p95 = percentile(&mut values, 95.0);
        let p99 = percentile(&mut values, 99.0);
        object.insert(
            name.to_string(),
            json!({ "p50": p50, "p95": p95, "p99": p99 }),
        );
    }
    row
}

fn micros(spans: &[Span], pick: impl Fn(&Span) -> Duration) -> Vec<f64> {
    spans
        .iter()
        .map(|span| pick(span).as_secs_f64() * 1_000_000.0)
        .collect()
}

fn percentile(samples: &mut [f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(|left, right| left.partial_cmp(right).expect("number"));
    let index = ((p / 100.0) * samples.len() as f64).ceil() as usize - 1;
    samples[index.min(samples.len() - 1)]
}

fn journal_row(bytes: usize) -> JournalRow {
    JournalRow {
        key: "echo".into(),
        idempotency_key: "echo:1".into(),
        status: "completed".into(),
        input: "x".repeat(bytes),
        result: "x".repeat(bytes),
    }
}

fn journal_wire(journal: &BTreeMap<String, JournalRow>) -> String {
    let mut raw = String::new();
    for (key, row) in journal {
        raw.push_str(&format!(
            "j\t{key}\t{}\t{}\t{}\t{}\n",
            row.idempotency_key, row.status, row.input, row.result
        ));
    }
    raw
}

fn continuation_with_live(bytes: usize) -> Continuation {
    let mut continuation = Continuation::start(
        "exec",
        "artifact",
        ENGINE_FORMAT_VERSION,
        "ts.subset.v1",
        0,
        0,
    );
    if bytes > 0 {
        continuation.stack.push(Value::String("x".repeat(bytes)));
    }
    continuation
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("deflate");
    encoder.finish().expect("deflate finish")
}

fn current_rss() -> u64 {
    let Ok(output) = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
    else {
        return 0;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<u64>().unwrap_or(0) * 1024
}

fn cpu_model() -> String {
    if let Ok(output) = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            return text;
        }
    }
    std::env::consts::ARCH.to_string()
}

fn scratch(label: &str) -> PathBuf {
    let path = bench_root().join(format!("b{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    path
}

fn bench_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root.pop();
    root.push("target");
    root.push("pulse");
    fs::create_dir_all(&root).expect("bench dir");
    root
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
            required_engine_features: current_features(),
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

fn current_features() -> Vec<EngineFeature> {
    tcc_ir::FeatureSet::current_engine()
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
