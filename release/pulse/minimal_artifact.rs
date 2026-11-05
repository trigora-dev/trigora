use sha2::{Digest, Sha256};
use std::env;
use std::fs;

fn main() {
    let destination = env::args().nth(1).expect("artifact path");
    let mut artifact = tcc_ir::Artifact::minimal_return(String::new());
    let canonical = tcc_ir::canonical_artifact_json(&artifact);
    let digest = Sha256::digest(canonical.as_bytes());
    artifact.envelope.artifact_hash = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    tcc_ir::validate(&artifact, &tcc_ir::EngineCaps::current()).expect("validate artifact");
    let encoded = tcc_ir::encode_artifact(&artifact).expect("encode artifact");
    fs::write(&destination, encoded).expect("write artifact");
    println!("{}", artifact.envelope.artifact_hash);
}
