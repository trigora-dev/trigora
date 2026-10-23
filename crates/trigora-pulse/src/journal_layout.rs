//! Bench-only journal storage layouts. Not used by the host commit path.
//!
//! Identity is the production `commit_identity`: revision, continuation digest,
//! and the full logical journal. Segment boundaries never enter that hash.

use std::collections::BTreeMap;

use crate::authority::{commit_identity, sha256, JournalRow};

const BOUND: usize = 32;
const GROW_COMMITS: usize = 4_000;
const LARGE_COMMITS: usize = 2_000;
const CHURN_COMMITS: usize = 2_000;
const CHURN_KEYS: usize = 64;
const QUIET_COMMITS: usize = 2_000;
const QUIET_KEYS: usize = 128;

#[derive(Clone, Copy)]
enum Layout {
    Full,
    Segments,
    BoundedChain,
    SnapshotDeltas,
}

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Segments => "segments",
            Self::BoundedChain => "bounded-chain",
            Self::SnapshotDeltas => "snapshot-deltas",
        }
    }

    fn all() -> [Self; 4] {
        [
            Self::Full,
            Self::Segments,
            Self::BoundedChain,
            Self::SnapshotDeltas,
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlobKind {
    Snapshot,
    Delta,
}

#[derive(Clone)]
struct Blob {
    len: u64,
    refs: u32,
    kind: BlobKind,
    rows: BTreeMap<String, JournalRow>,
}

#[derive(Clone)]
struct Revision {
    identity: String,
    owner: String,
    continuation_digest: String,
    blobs: Vec<String>,
}

#[derive(Clone)]
struct Sim {
    layout: Layout,
    keep_rows: bool,
    revision: u64,
    identity: Option<String>,
    owner: String,
    continuation: Vec<u8>,
    logical: BTreeMap<String, JournalRow>,
    logical_wire: u64,
    fingerprint: String,
    blobs: BTreeMap<String, Blob>,
    revisions: Vec<Revision>,
    journal_transmitted: u64,
    max_journal_commit: u64,
    logical_bytes_hashed: u64,
    objects_written: u64,
    max_cold_reads: u64,
    continuation_objects: u64,
    continuation_bytes: u64,
    continuations: BTreeMap<String, u64>,
}

#[derive(Debug)]
enum Outcome {
    Applied,
    Idempotent,
}

#[derive(Debug)]
enum LayoutError {
    Conflict,
    DeletedKey(String),
}

struct LayoutStats {
    transmitted: u64,
    max_commit: u64,
    objects_written: u64,
    cold_reads: u64,
    max_cold_reads: u64,
    live_bytes: u64,
    live_objects: u64,
    obsolete_bytes: u64,
    obsolete_objects: u64,
    latest_bytes: u64,
    compact_live_bytes: u64,
    compact_obsolete_bytes: u64,
    compact_obsolete_objects: u64,
    dropped_live_bytes: u64,
    dropped_obsolete_bytes: u64,
    dropped_obsolete_objects: u64,
    cold_reads_after_compact: u64,
}

struct WorkloadStats {
    name: &'static str,
    commits: u64,
    local_bytes_hashed: u64,
    continuation_objects: u64,
    continuation_bytes: u64,
    layouts: Vec<(&'static str, LayoutStats)>,
}

fn row_line(key: &str, row: &JournalRow) -> String {
    format!(
        "j\t{key}\t{}\t{}\t{}\t{}\n",
        row.idempotency_key, row.status, row.input, row.result
    )
}

fn row_line_len(key: &str, row: &JournalRow) -> u64 {
    row_line(key, row).len() as u64
}

fn wire_len(journal: &BTreeMap<String, JournalRow>) -> u64 {
    journal
        .iter()
        .map(|(key, row)| row_line_len(key, row))
        .sum()
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

/// New and replaced rows. A key that disappeared is rejected. Equal rows are omitted.
fn diff(
    previous: &BTreeMap<String, JournalRow>,
    next: &BTreeMap<String, JournalRow>,
) -> Result<BTreeMap<String, JournalRow>, LayoutError> {
    for key in previous.keys() {
        if !next.contains_key(key) {
            return Err(LayoutError::DeletedKey(key.clone()));
        }
    }
    let mut changed = BTreeMap::new();
    for (key, row) in next {
        match previous.get(key) {
            None => {
                changed.insert(key.clone(), row.clone());
            }
            Some(old) if old != row => {
                changed.insert(key.clone(), row.clone());
            }
            Some(_) => {}
        }
    }
    Ok(changed)
}

fn row(key: &str, status: &str, input: &str, result: &str) -> JournalRow {
    JournalRow {
        key: key.to_string(),
        idempotency_key: format!("exec:{key}"),
        status: status.to_string(),
        input: input.to_string(),
        result: result.to_string(),
    }
}

fn fill(bytes: usize, mark: u8) -> String {
    String::from_utf8(vec![mark; bytes]).expect("ascii fill")
}

impl Sim {
    fn new(layout: Layout, keep_rows: bool) -> Self {
        Self {
            layout,
            keep_rows,
            revision: 0,
            identity: None,
            owner: String::new(),
            continuation: Vec::new(),
            logical: BTreeMap::new(),
            logical_wire: 0,
            fingerprint: sha256(b"journal"),
            blobs: BTreeMap::new(),
            revisions: Vec::new(),
            journal_transmitted: 0,
            max_journal_commit: 0,
            logical_bytes_hashed: 0,
            objects_written: 0,
            max_cold_reads: 0,
            continuation_objects: 0,
            continuation_bytes: 0,
            continuations: BTreeMap::new(),
        }
    }

    fn commit(
        &mut self,
        expected: u64,
        owner: &str,
        continuation: &[u8],
        journal: &BTreeMap<String, JournalRow>,
    ) -> Result<Outcome, LayoutError> {
        let next = expected + 1;
        let digest = sha256(continuation);
        // The production hash stays the identity whenever rows are retained.
        // The long measurement counts the same bytes without building that string.
        let identity = if self.keep_rows {
            Some(commit_identity(next, &digest, journal, &[], &[]))
        } else {
            None
        };
        if self.revision == next {
            if self.identity.as_deref() == Some(identity.as_deref().unwrap_or("")) {
                return Ok(Outcome::Idempotent);
            }
            return Err(LayoutError::Conflict);
        }
        if self.revision != expected {
            return Err(LayoutError::Conflict);
        }
        let delta = diff(&self.logical, journal)?;
        self.note_continuation(continuation);
        self.fold_delta(&delta);
        for (key, row) in &delta {
            self.logical.insert(key.clone(), row.clone());
        }
        self.logical_bytes_hashed += self.logical_wire;
        let blobs = self.place(&delta);
        self.attach(blobs);
        let identity = identity.unwrap_or_else(|| self.fingerprint.clone());
        self.revision = next;
        self.identity = Some(identity.clone());
        self.owner = owner.to_string();
        self.continuation = continuation.to_vec();
        let revision = self.revisions.last_mut().expect("revision");
        revision.identity = identity;
        revision.owner = owner.to_string();
        revision.continuation_digest = digest;
        Ok(Outcome::Applied)
    }

    fn fold_delta(&mut self, delta: &BTreeMap<String, JournalRow>) {
        for (key, row) in delta {
            let old = self
                .logical
                .get(key)
                .map(|existing| row_line(key, existing))
                .unwrap_or_default();
            if let Some(existing) = self.logical.get(key) {
                self.logical_wire -= row_line_len(key, existing);
            }
            self.logical_wire += row_line_len(key, row);
            let new = row_line(key, row);
            self.fingerprint = sha256(format!("{}\n{old}\n{new}", self.fingerprint).as_bytes());
        }
    }

    fn place(&mut self, delta: &BTreeMap<String, JournalRow>) -> Vec<String> {
        let previous = self
            .revisions
            .last()
            .map(|revision| revision.blobs.clone())
            .unwrap_or_default();
        match self.layout {
            Layout::Full => self.store_snapshot(),
            Layout::Segments => self.append_delta(&previous, delta),
            Layout::BoundedChain => {
                if delta.is_empty() {
                    self.note_transmit(0);
                    previous
                } else if previous.len() >= BOUND {
                    self.store_snapshot()
                } else {
                    self.append_delta(&previous, delta)
                }
            }
            Layout::SnapshotDeltas => {
                if delta.is_empty() {
                    self.note_transmit(0);
                    previous
                } else if previous.is_empty() || previous.len() > BOUND {
                    // One snapshot plus at most BOUND deltas. The next change
                    // rewrites the snapshot and clears the deltas.
                    self.store_snapshot()
                } else {
                    self.append_delta(&previous, delta)
                }
            }
        }
    }

    fn append_delta(
        &mut self,
        previous: &[String],
        delta: &BTreeMap<String, JournalRow>,
    ) -> Vec<String> {
        if delta.is_empty() {
            self.note_transmit(0);
            return previous.to_vec();
        }
        let len = wire_len(delta);
        self.note_transmit(len as usize);
        let digest = sha256(journal_wire(delta).as_bytes());
        let mut blobs = previous.to_vec();
        blobs.push(self.store(BlobKind::Delta, len, digest, delta));
        blobs
    }

    fn store_snapshot(&mut self) -> Vec<String> {
        self.note_transmit(self.logical_wire as usize);
        let digest = self.fingerprint.clone();
        let len = self.logical_wire;
        let rows = if self.keep_rows {
            self.logical.clone()
        } else {
            BTreeMap::new()
        };
        vec![self.store(BlobKind::Snapshot, len, digest, &rows)]
    }

    fn store(
        &mut self,
        kind: BlobKind,
        len: u64,
        digest: String,
        rows: &BTreeMap<String, JournalRow>,
    ) -> String {
        if self.blobs.contains_key(&digest) {
            return digest;
        }
        self.objects_written += 1;
        self.blobs.insert(
            digest.clone(),
            Blob {
                len,
                refs: 0,
                kind,
                rows: if self.keep_rows {
                    rows.clone()
                } else {
                    BTreeMap::new()
                },
            },
        );
        digest
    }

    fn attach(&mut self, blobs: Vec<String>) {
        for digest in &blobs {
            self.blobs.get_mut(digest).expect("blob").refs += 1;
        }
        self.max_cold_reads = self.max_cold_reads.max(blobs.len() as u64);
        self.revisions.push(Revision {
            identity: String::new(),
            owner: String::new(),
            continuation_digest: String::new(),
            blobs,
        });
    }

    fn note_transmit(&mut self, bytes: usize) {
        let bytes = bytes as u64;
        self.journal_transmitted += bytes;
        self.max_journal_commit = self.max_journal_commit.max(bytes);
    }

    fn note_continuation(&mut self, bytes: &[u8]) {
        let digest = sha256(bytes);
        if self.continuations.contains_key(&digest) {
            return;
        }
        let len = bytes.len() as u64;
        self.continuations.insert(digest, len);
        self.continuation_objects += 1;
        self.continuation_bytes += len;
    }

    fn materialize(&self) -> Option<BTreeMap<String, JournalRow>> {
        let revision = self.revisions.last()?;
        let mut map = BTreeMap::new();
        for digest in &revision.blobs {
            let blob = self.blobs.get(digest)?;
            if !self.keep_rows {
                return None;
            }
            match blob.kind {
                BlobKind::Snapshot => map.clone_from(&blob.rows),
                BlobKind::Delta => {
                    for (key, row) in &blob.rows {
                        map.insert(key.clone(), row.clone());
                    }
                }
            }
        }
        Some(map)
    }

    /// Rewrite the latest manifest to one snapshot of the logical journal.
    /// The stored identity, owner, and revision stay as they were.
    fn compact_latest(&mut self) {
        let digest = self.fingerprint.clone();
        let len = self.logical_wire;
        let rows = if self.keep_rows {
            self.logical.clone()
        } else {
            BTreeMap::new()
        };
        let digest = self.store(BlobKind::Snapshot, len, digest, &rows);
        let old = {
            let revision = self.revisions.last_mut().expect("revision");
            std::mem::replace(&mut revision.blobs, vec![digest.clone()])
        };
        for previous in old {
            let blob = self.blobs.get_mut(&previous).expect("blob");
            blob.refs = blob.refs.saturating_sub(1);
        }
        self.blobs.get_mut(&digest).expect("snapshot").refs += 1;
    }

    fn retain_latest_only(&mut self) {
        if self.revisions.len() < 2 {
            return;
        }
        let last = self.revisions.len() - 1;
        let old = self.revisions[..last]
            .iter()
            .flat_map(|revision| revision.blobs.clone())
            .collect::<Vec<_>>();
        for revision in &mut self.revisions[..last] {
            revision.blobs.clear();
        }
        for digest in old {
            let blob = self.blobs.get_mut(&digest).expect("blob");
            blob.refs = blob.refs.saturating_sub(1);
        }
    }

    fn cold_reads(&self) -> u64 {
        self.revisions
            .last()
            .map(|revision| revision.blobs.len() as u64)
            .unwrap_or(0)
    }

    fn named_bytes(&self, digests: &[String]) -> u64 {
        let mut seen = BTreeMap::<&str, ()>::new();
        let mut bytes = 0;
        for digest in digests {
            if seen.insert(digest.as_str(), ()).is_some() {
                continue;
            }
            bytes += self.blobs.get(digest).map(|blob| blob.len).unwrap_or(0);
        }
        bytes
    }

    fn occupancy(&self) -> (u64, u64, u64, u64) {
        let mut live_bytes = 0;
        let mut live_objects = 0;
        let mut obsolete_bytes = 0;
        let mut obsolete_objects = 0;
        for blob in self.blobs.values() {
            if blob.refs == 0 {
                obsolete_bytes += blob.len;
                obsolete_objects += 1;
            } else {
                live_bytes += blob.len;
                live_objects += 1;
            }
        }
        (live_bytes, live_objects, obsolete_bytes, obsolete_objects)
    }

    fn stats(&self) -> LayoutStats {
        let (live_bytes, live_objects, obsolete_bytes, obsolete_objects) = self.occupancy();
        let latest_bytes = self
            .revisions
            .last()
            .map(|revision| self.named_bytes(&revision.blobs))
            .unwrap_or(0);
        let mut compacted = self.clone();
        compacted.compact_latest();
        let (compact_live_bytes, _, compact_obsolete_bytes, compact_obsolete_objects) =
            compacted.occupancy();
        let cold_reads_after_compact = compacted.cold_reads();
        compacted.retain_latest_only();
        let (_, _, dropped_obsolete_bytes, dropped_obsolete_objects) = compacted.occupancy();
        let dropped_live_bytes = compacted
            .revisions
            .last()
            .map(|revision| compacted.named_bytes(&revision.blobs))
            .unwrap_or(0);
        LayoutStats {
            transmitted: self.journal_transmitted,
            max_commit: self.max_journal_commit,
            objects_written: self.objects_written,
            cold_reads: self.cold_reads(),
            max_cold_reads: self.max_cold_reads,
            live_bytes,
            live_objects,
            obsolete_bytes,
            obsolete_objects,
            latest_bytes,
            compact_live_bytes,
            compact_obsolete_bytes,
            compact_obsolete_objects,
            dropped_live_bytes,
            dropped_obsolete_bytes,
            dropped_obsolete_objects,
            cold_reads_after_compact,
        }
    }
}

fn drive_grow(layout: Layout, commits: usize, row_bytes: usize, keep_rows: bool) -> Sim {
    let mut sim = Sim::new(layout, keep_rows);
    let mut journal = BTreeMap::new();
    let continuation = b"continuation".to_vec();
    for index in 0..commits {
        let key = format!("k{index:05}");
        journal.insert(
            key.clone(),
            row(
                &key,
                "completed",
                &fill(row_bytes, b'i'),
                &fill(row_bytes, b'o'),
            ),
        );
        let outcome = sim
            .commit(sim.revision, "owner", &continuation, &journal)
            .expect("grow");
        assert!(matches!(outcome, Outcome::Applied));
    }
    sim
}

fn drive_churn(layout: Layout, keep_rows: bool) -> Sim {
    let mut sim = Sim::new(layout, keep_rows);
    let mut journal = BTreeMap::new();
    let continuation = b"continuation".to_vec();
    for index in 0..CHURN_KEYS {
        let key = format!("k{index:05}");
        journal.insert(key.clone(), row(&key, "started", &fill(128, b'i'), ""));
    }
    apply(&mut sim, &continuation, &journal);
    for index in 0..CHURN_COMMITS {
        let key = format!("k{:05}", index % CHURN_KEYS);
        let current = journal.get_mut(&key).expect("key");
        if current.status == "started" {
            current.status = "completed".to_string();
            current.result = "done".to_string();
        } else {
            current.result = format!("r{index}");
        }
        apply(&mut sim, &continuation, &journal);
    }
    sim
}

fn drive_quiet(layout: Layout, keep_rows: bool) -> Sim {
    let mut sim = Sim::new(layout, keep_rows);
    let mut journal = BTreeMap::new();
    for index in 0..QUIET_KEYS {
        let key = format!("k{index:05}");
        journal.insert(
            key.clone(),
            row(&key, "completed", &fill(64, b'i'), &fill(64, b'o')),
        );
    }
    for index in 0..QUIET_COMMITS {
        let continuation = format!("c{index}").into_bytes();
        apply(&mut sim, &continuation, &journal);
    }
    sim
}

fn apply(sim: &mut Sim, continuation: &[u8], journal: &BTreeMap<String, JournalRow>) {
    let outcome = sim
        .commit(sim.revision, "owner", continuation, journal)
        .expect("commit");
    assert!(matches!(outcome, Outcome::Applied));
}

fn workload(name: &'static str, run: impl Fn(Layout) -> Sim) -> WorkloadStats {
    let mut layouts = Vec::new();
    let mut sample: Option<Sim> = None;
    for layout in Layout::all() {
        let sim = run(layout);
        sample.get_or_insert_with(|| sim.clone());
        layouts.push((layout.name(), sim.stats()));
    }
    let sample = sample.expect("layout");
    WorkloadStats {
        name,
        commits: sample.revision,
        local_bytes_hashed: sample.logical_bytes_hashed,
        continuation_objects: sample.continuation_objects,
        continuation_bytes: sample.continuation_bytes,
        layouts,
    }
}

fn review() -> Vec<WorkloadStats> {
    vec![
        workload("small-grow", |layout| {
            drive_grow(layout, GROW_COMMITS, 32, false)
        }),
        workload("large-grow", |layout| {
            drive_grow(layout, LARGE_COMMITS, 1024, false)
        }),
        workload("churn", |layout| drive_churn(layout, false)),
        workload("continuation-only", |layout| drive_quiet(layout, false)),
    ]
}

fn format_review(workloads: &[WorkloadStats]) -> String {
    let mut out = String::new();
    out.push_str("journal layout review\n");
    out.push_str(&format!(
        "delta bound {BOUND}; identity is revision + continuation digest + full logical journal\n"
    ));
    out.push_str(
        "local cost is the logical journal bytes hashed on every commit, in every layout\n",
    );
    out.push_str("cold reads are journal objects named by the latest revision\n");
    out.push_str(
        "compact rewrites only the latest manifest; history-retained obsolete bytes stay referenced by older revisions\n",
    );
    for workload in workloads {
        out.push_str(&format!(
            "\n{} commits={} local_journal_bytes_hashed={} continuation_objects={} continuation_bytes={}\n",
            workload.name,
            workload.commits,
            workload.local_bytes_hashed,
            workload.continuation_objects,
            workload.continuation_bytes,
        ));
        for (name, stats) in &workload.layouts {
            out.push_str(&format!(
                "  {name} transmitted={} max_commit={} objects={} cold={} cold_max={} live_bytes={} live_objects={} obsolete_bytes={} obsolete_objects={} latest_bytes={}\n",
                stats.transmitted,
                stats.max_commit,
                stats.objects_written,
                stats.cold_reads,
                stats.max_cold_reads,
                stats.live_bytes,
                stats.live_objects,
                stats.obsolete_bytes,
                stats.obsolete_objects,
                stats.latest_bytes,
            ));
            out.push_str(&format!(
                "    after_compact history_retained live_bytes={} obsolete_bytes={} obsolete_objects={} cold={}\n",
                stats.compact_live_bytes,
                stats.compact_obsolete_bytes,
                stats.compact_obsolete_objects,
                stats.cold_reads_after_compact,
            ));
            out.push_str(&format!(
                "    after_compact latest_only live_bytes={} obsolete_bytes={} obsolete_objects={}\n",
                stats.dropped_live_bytes,
                stats.dropped_obsolete_bytes,
                stats.dropped_obsolete_objects,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_pair(commits: usize) -> (Sim, Sim) {
        let full = drive_grow(Layout::Full, commits, 8, true);
        let snapshot = drive_grow(Layout::SnapshotDeltas, commits, 8, true);
        (full, snapshot)
    }

    #[test]
    fn delta_rules_insert_replace_and_reject_deletion() {
        let mut previous = BTreeMap::new();
        previous.insert("k00000".to_string(), row("k00000", "started", "in", ""));
        let mut next = previous.clone();
        next.insert(
            "k00001".to_string(),
            row("k00001", "completed", "in", "out"),
        );
        let changed = diff(&previous, &next).expect("insert");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed["k00001"].status, "completed");

        next.get_mut("k00000").expect("row").status = "completed".to_string();
        next.get_mut("k00000").expect("row").result = "out".to_string();
        let changed = diff(&previous, &next).expect("replace");
        assert_eq!(changed.len(), 2);
        assert_eq!(changed["k00000"].status, "completed");

        let unchanged = diff(&next, &next).expect("same");
        assert!(unchanged.is_empty());
        assert_eq!(journal_wire(&next).len() as u64, wire_len(&next));

        let mut dropped = next.clone();
        dropped.remove("k00000");
        assert!(matches!(
            diff(&next, &dropped),
            Err(LayoutError::DeletedKey(key)) if key == "k00000"
        ));
    }

    #[test]
    fn later_segment_wins_and_started_becomes_completed() {
        let mut sim = Sim::new(Layout::Segments, true);
        let continuation = b"continuation".to_vec();
        let mut journal = BTreeMap::new();
        journal.insert("k00000".to_string(), row("k00000", "started", "in", ""));
        apply(&mut sim, &continuation, &journal);
        journal.get_mut("k00000").expect("row").status = "completed".to_string();
        journal.get_mut("k00000").expect("row").result = "out".to_string();
        apply(&mut sim, &continuation, &journal);
        let map = sim.materialize().expect("rows");
        assert_eq!(map.len(), 1);
        assert_eq!(map["k00000"].status, "completed");
        assert_eq!(map["k00000"].result, "out");

        let mut reversed = BTreeMap::new();
        let first = journal_wire(&{
            let mut only = BTreeMap::new();
            only.insert("k00000".to_string(), row("k00000", "started", "in", ""));
            only
        });
        let second = journal_wire(&{
            let mut only = BTreeMap::new();
            only.insert(
                "k00000".to_string(),
                row("k00000", "completed", "in", "out"),
            );
            only
        });
        assert_ne!(first, second);
        reversed.insert(
            "k00000".to_string(),
            row("k00000", "completed", "in", "out"),
        );
        reversed.insert("k00000".to_string(), row("k00000", "started", "in", ""));
        assert_eq!(reversed["k00000"].status, "started");
    }

    #[test]
    fn layouts_share_logical_identity_and_reconstruct_the_journal() {
        let (full, snapshot) = layout_pair(40);
        assert_eq!(full.identity, snapshot.identity);
        assert_eq!(full.logical, snapshot.materialize().expect("snapshot rows"));
        let segments = drive_grow(Layout::Segments, 40, 8, true);
        let chain = drive_grow(Layout::BoundedChain, 40, 8, true);
        assert_eq!(full.identity, segments.identity);
        assert_eq!(full.identity, chain.identity);
        assert_eq!(full.logical, segments.materialize().expect("segment rows"));
        assert_eq!(full.logical, chain.materialize().expect("chain rows"));
        for revision in 0..full.revisions.len() {
            assert_eq!(
                full.revisions[revision].identity,
                snapshot.revisions[revision].identity
            );
            assert_eq!(
                full.revisions[revision].identity,
                segments.revisions[revision].identity
            );
        }
    }

    #[test]
    fn compact_then_retry_is_idempotent_and_a_changed_journal_conflicts() {
        let mut sim = drive_grow(Layout::SnapshotDeltas, 40, 8, true);
        let identity = sim.identity.clone().expect("identity");
        let owner = sim.owner.clone();
        let revision = sim.revision;
        sim.compact_latest();
        assert_eq!(sim.identity.as_deref(), Some(identity.as_str()));
        assert_eq!(sim.owner, owner);
        assert_eq!(sim.revision, revision);
        assert_eq!(sim.cold_reads(), 1);
        assert_eq!(sim.materialize().expect("compacted"), sim.logical);
        let outcome = sim
            .commit(
                sim.revision - 1,
                "intruder",
                &sim.continuation.clone(),
                &sim.logical.clone(),
            )
            .expect("retry");
        assert!(matches!(outcome, Outcome::Idempotent));
        assert_eq!(sim.owner, owner);
        assert_eq!(sim.revision, revision);
        assert_eq!(sim.identity.as_deref(), Some(identity.as_str()));

        let continuation = sim.continuation.clone();
        let mut tampered = sim.logical.clone();
        tampered.get_mut("k00000").expect("row").result = "tampered".to_string();
        let err = sim
            .commit(sim.revision - 1, "owner", &continuation, &tampered)
            .expect_err("conflict");
        assert!(matches!(err, LayoutError::Conflict));
        assert_eq!(sim.identity.as_deref(), Some(identity.as_str()));
        assert_eq!(sim.logical["k00000"].result, fill(8, b'o'));
    }

    #[test]
    fn a_new_revision_cannot_drop_a_committed_key() {
        let mut sim = drive_grow(Layout::Segments, 4, 4, true);
        let mut dropped = sim.logical.clone();
        dropped.remove("k00000");
        let err = sim
            .commit(sim.revision, "owner", &sim.continuation.clone(), &dropped)
            .expect_err("delete");
        assert!(matches!(err, LayoutError::DeletedKey(key) if key == "k00000"));
        assert!(sim.logical.contains_key("k00000"));
    }

    #[test]
    fn review_bounds_recovery_and_cuts_repeated_uploads() {
        let workloads = review();
        let grow = workloads
            .iter()
            .find(|workload| workload.name == "small-grow")
            .expect("grow");
        let full = layout(grow, "full");
        let segments = layout(grow, "segments");
        let chain = layout(grow, "bounded-chain");
        let snapshot = layout(grow, "snapshot-deltas");
        assert!(segments.transmitted * 10 < full.transmitted);
        assert!(snapshot.transmitted * 10 < full.transmitted);
        assert_eq!(segments.max_cold_reads, grow.commits);
        assert!(snapshot.max_cold_reads <= (BOUND as u64) + 1);
        assert!(chain.max_cold_reads <= BOUND as u64);
        assert!(snapshot.max_commit > segments.max_commit);
        assert_eq!(full.obsolete_bytes, 0);
        assert!(snapshot.dropped_obsolete_bytes > snapshot.compact_obsolete_bytes);
        assert!(snapshot.compact_live_bytes > 0);
        assert_eq!(snapshot.cold_reads_after_compact, 1);

        let quiet = workloads
            .iter()
            .find(|workload| workload.name == "continuation-only")
            .expect("quiet");
        let quiet_full = layout(quiet, "full");
        let quiet_segments = layout(quiet, "segments");
        assert_eq!(quiet_segments.transmitted, quiet_segments.max_commit);
        assert!(quiet_full.transmitted > quiet_segments.transmitted * 100);
        assert_eq!(quiet_segments.objects_written, 1);

        let churn = workloads
            .iter()
            .find(|workload| workload.name == "churn")
            .expect("churn");
        assert!(layout(churn, "segments").transmitted * 10 < layout(churn, "full").transmitted);
        println!("{}", format_review(&workloads));
    }

    fn layout<'a>(workload: &'a WorkloadStats, name: &str) -> &'a LayoutStats {
        &workload
            .layouts
            .iter()
            .find(|(layout, _)| *layout == name)
            .expect(name)
            .1
    }
}
