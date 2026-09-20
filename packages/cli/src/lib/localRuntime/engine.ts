import { randomUUID } from 'node:crypto';

import { resumeExecution, startExecution, Store, type RunResult } from '@tcc-engine/host-node';
import type { ApiErrorCode, Execution, ExecutionStatus, JsonValue } from '@trigora/contracts';
import { DEFAULT_PROJECT_SLUG, ENGINE_FORMAT_VERSION } from '@trigora/contracts';

import type { DiscoveredProgram } from './discoverPrograms';
import { decodeTagged, toJsonValue } from './tagged';

export class LocalRuntimeError extends Error {
  readonly code: ApiErrorCode;

  constructor(code: ApiErrorCode, message: string) {
    super(message);
    this.name = 'LocalRuntimeError';
    this.code = code;
  }
}

export type EngineListener = (event: {
  type: 'started' | 'waiting' | 'resumed' | 'completed' | 'failed' | 'cancelled' | 'effect';
  execution: Execution;
  name?: string;
}) => void;

const OWNER_TOKEN = 'trigora-dev';
const LEASE_MS = 60 * 60 * 1000;
const LOCAL_PROJECT_ID = DEFAULT_PROJECT_SLUG;

function localExecutionFields(programId: string, artifactHash = ''): Pick<
  Execution,
  'projectId' | 'programName' | 'programVersionId' | 'artifactHash' | 'engineFormatVersion'
> {
  return {
    projectId: LOCAL_PROJECT_ID,
    programName: programId,
    programVersionId: artifactHash || programId,
    artifactHash,
    engineFormatVersion: ENGINE_FORMAT_VERSION,
  };
}

type ExecutionRow = {
  id: string;
  artifact_hash: string;
  revision: number;
  status: string;
  owner_token: string | null;
  lease_until: number | null;
};

type MetaRow = {
  id: string;
  program_id: string;
  input_json: string;
  created_at: string;
  updated_at: string;
};

function now(): string {
  return new Date().toISOString();
}

function mapStatus(status: string): ExecutionStatus {
  if (status === 'suspended') {
    return 'waiting';
  }
  if (status === 'runnable') {
    return 'running';
  }
  if (status === 'completed' || status === 'failed' || status === 'cancelled') {
    return status;
  }
  return 'running';
}

export class LocalExecutionEngine {
  private readonly programs = new Map<string, DiscoveredProgram>();
  private readonly artifacts = new Map<string, DiscoveredProgram>();

  constructor(
    private readonly dbPath: string,
    private readonly onEvent?: EngineListener,
  ) {
    this.withStore((store) => {
      this.ensureMeta(store);
    });
  }

  listPrograms(): DiscoveredProgram[] {
    return [...this.programs.values()];
  }

  getProgram(programId: string): DiscoveredProgram | undefined {
    return this.programs.get(programId);
  }

  replacePrograms(programs: DiscoveredProgram[]): void {
    this.programs.clear();
    for (const program of programs) {
      this.programs.set(program.id, program);
      this.artifacts.set(program.artifactHash, program);
    }
  }

  restoredWaiting(): Execution[] {
    return this.withStore((store) => {
      const rows = store.db
        .prepare("SELECT * FROM executions WHERE status = 'suspended'")
        .all() as ExecutionRow[];
      return rows.map((row) => this.toRecord(store, row));
    });
  }

  getExecution(executionId: string): Execution {
    return this.withStore((store) =>
      this.toRecord(store, this.requireExecution(store, executionId)),
    );
  }

  listExecutions(): Execution[] {
    return this.withStore((store) => {
      this.ensureMeta(store);
      const rows = store.db.prepare('SELECT * FROM executions').all() as ExecutionRow[];
      return rows
        .map((row) => this.toRecord(store, row))
        .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
    });
  }

  async start(programId: string, input: unknown): Promise<Execution> {
    const program = this.programs.get(programId);
    if (!program) {
      throw new LocalRuntimeError('program_not_found', `Program "${programId}" was not found.`);
    }

    const id = `exec_local_${randomUUID()}`;
    const createdAt = now();
    this.withStore((store) => {
      this.ensureMeta(store);
      store.db
        .prepare(
          'INSERT INTO trigora_records(id, program_id, input_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?)',
        )
        .run(id, programId, JSON.stringify(toJsonValue(input)), createdAt, createdAt);
    });

    const started: Execution = {
      id,
      ...localExecutionFields(programId, program.artifactHash),
      programId,
      status: 'running',
      input: toJsonValue(input),
      attempt: 1,
      createdAt,
      updatedAt: createdAt,
    };
    this.onEvent?.({ type: 'started', execution: started });

    const result = await startExecution({
      dbPath: this.dbPath,
      artifactJson: program.artifactJson,
      executionId: id,
      ownerToken: OWNER_TOKEN,
      leaseMs: LEASE_MS,
      autoDeliverEvent: false,
      runEffect: this.createEffectRunner(program, id),
    });

    const record = this.recordFromResult(id, result);
    this.emitTerminal(record, { waiting: true });
    return record;
  }

  async send(executionId: string, name: string, payload: unknown): Promise<Execution> {
    const current = this.getExecution(executionId);
    if (current.status !== 'waiting' || current.wait?.type !== 'event') {
      throw new LocalRuntimeError(
        'execution_not_waiting',
        `Execution "${executionId}" is not waiting for an event.`,
      );
    }

    if (current.wait.event !== name) {
      throw new LocalRuntimeError(
        'event_mismatch',
        `Execution "${executionId}" is waiting for event "${current.wait.event}".`,
      );
    }

    this.onEvent?.({
      type: 'resumed',
      execution: { ...current, status: 'running', wait: undefined },
    });

    const program = this.programForExecution(executionId);
    const result = await resumeExecution({
      dbPath: this.dbPath,
      executionId,
      ownerToken: OWNER_TOKEN,
      leaseMs: LEASE_MS,
      autoDeliverEvent: true,
      eventPayload: payload,
      runEffect: this.createEffectRunner(program, executionId),
    });

    const record = this.recordFromResult(executionId, result);
    this.emitTerminal(record);
    return record;
  }

  async cancel(executionId: string): Promise<Execution> {
    const current = this.getExecution(executionId);
    if (current.status === 'completed' || current.status === 'failed') {
      throw new LocalRuntimeError(
        'execution_not_cancellable',
        `Execution "${executionId}" is already ${current.status}.`,
      );
    }

    if (current.status === 'cancelled') {
      return current;
    }

    const program = this.programForExecution(executionId);
    const result = await resumeExecution({
      dbPath: this.dbPath,
      executionId,
      ownerToken: OWNER_TOKEN,
      leaseMs: LEASE_MS,
      cancel: true,
      autoDeliverEvent: false,
      runEffect: this.createEffectRunner(program, executionId),
    });

    const record = this.recordFromResult(executionId, result);
    if (record.status !== 'cancelled') {
      this.withStore((store) => {
        store.db
          .prepare("UPDATE executions SET status = 'cancelled' WHERE id = ?")
          .run(executionId);
        store.db
          .prepare('UPDATE trigora_records SET updated_at = ? WHERE id = ?')
          .run(now(), executionId);
      });
      const cancelled = {
        ...record,
        status: 'cancelled' as const,
        wait: undefined,
        updatedAt: now(),
      };
      this.onEvent?.({ type: 'cancelled', execution: cancelled });
      return cancelled;
    }

    this.onEvent?.({ type: 'cancelled', execution: record });
    return record;
  }

  private programForExecution(executionId: string): DiscoveredProgram {
    const hash = this.withStore((store) => this.requireExecution(store, executionId).artifact_hash);
    const pinned = this.artifacts.get(hash);
    if (pinned) {
      return pinned;
    }

    const programId = this.withStore((store) => {
      const meta = store.db
        .prepare('SELECT program_id FROM trigora_records WHERE id = ?')
        .get(executionId) as { program_id: string } | undefined;
      return meta?.program_id;
    });
    const current = programId ? this.programs.get(programId) : undefined;
    if (!current) {
      throw new LocalRuntimeError(
        'execution_not_found',
        `Execution "${executionId}" was not found.`,
      );
    }
    return current;
  }

  private createEffectRunner(program: DiscoveredProgram, executionId: string) {
    return (key: string) => {
      const handler = program.effects[key];
      if (!handler) {
        throw new Error(`No effect handler for \`${key}\`.`);
      }
      this.onEvent?.({
        type: 'effect',
        name: key,
        execution: {
          id: executionId,
          ...localExecutionFields(program.id, program.artifactHash),
          programId: program.id,
          status: 'running',
          input: null,
          attempt: 1,
          createdAt: now(),
          updatedAt: now(),
        },
      });
      return handler();
    };
  }

  private recordFromResult(executionId: string, _result: RunResult): Execution {
    this.withStore((store) => {
      store.db
        .prepare('UPDATE trigora_records SET updated_at = ? WHERE id = ?')
        .run(now(), executionId);
    });
    return this.getExecution(executionId);
  }

  private emitTerminal(record: Execution, options?: { waiting?: boolean }): void {
    if (record.status === 'waiting' && options?.waiting) {
      this.onEvent?.({ type: 'waiting', execution: record });
      return;
    }
    if (record.status === 'completed') {
      this.onEvent?.({ type: 'completed', execution: record });
    }
    if (record.status === 'failed') {
      this.onEvent?.({ type: 'failed', execution: record });
    }
    if (record.status === 'cancelled') {
      this.onEvent?.({ type: 'cancelled', execution: record });
    }
  }

  private toRecord(store: Store, row: ExecutionRow): Execution {
    const meta = store.db.prepare('SELECT * FROM trigora_records WHERE id = ?').get(row.id) as
      | MetaRow
      | undefined;
    const saved = store.getContinuation(row.id);
    const parsed = saved
      ? (JSON.parse(saved.json) as { result?: unknown; status?: string })
      : undefined;
    const wait = store.pendingWait(row.id);
    const timer = store.pendingTimer(row.id);
    const status = mapStatus(row.status);
    const result =
      parsed?.result !== undefined ? toJsonValue(decodeTagged(parsed.result)) : undefined;
    const failedMessage =
      status === 'failed' && typeof parsed?.result === 'string' ? parsed.result : undefined;

    const programId = meta?.program_id ?? 'unknown';
    return {
      id: row.id,
      ...localExecutionFields(programId, row.artifact_hash),
      programId,
      status,
      input: (meta ? (JSON.parse(meta.input_json) as JsonValue) : null) ?? null,
      result: status === 'completed' ? result : undefined,
      error:
        status === 'failed'
          ? { name: 'Error', message: failedMessage ?? 'Execution failed.' }
          : undefined,
      wait:
        status === 'waiting'
          ? wait
            ? { type: 'event', event: wait.event_name }
            : timer
              ? { type: 'timer', wakeAt: new Date(timer.resume_at_ms).toISOString() }
              : undefined
          : undefined,
      attempt: 1,
      createdAt: meta?.created_at ?? now(),
      updatedAt: meta?.updated_at ?? now(),
    };
  }

  private requireExecution(store: Store, executionId: string): ExecutionRow {
    const row = store.getExecution(executionId) as ExecutionRow | undefined;
    if (!row) {
      throw new LocalRuntimeError(
        'execution_not_found',
        `Execution "${executionId}" was not found.`,
      );
    }
    return row;
  }

  private ensureMeta(store: Store): void {
    store.db.exec(`
      CREATE TABLE IF NOT EXISTS trigora_records (
        id TEXT PRIMARY KEY,
        program_id TEXT NOT NULL,
        input_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
    `);
  }

  private withStore<T>(fn: (store: Store) => T): T {
    const store = new Store(this.dbPath);
    try {
      return fn(store);
    } finally {
      store.close();
    }
  }
}
