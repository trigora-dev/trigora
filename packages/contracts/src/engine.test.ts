import { describe, expect, it } from 'vitest';

import type {
  CompileRequest,
  CompileResult,
  EventDefinition,
  ExecutionRecord,
  ListProgramsResponse,
  ProgramIdentity,
  StartExecutionRequest,
  TrigoraConfig,
  WaitCondition,
} from './index';
import { DEFAULT_RUNTIME_HOST, DEFAULT_RUNTIME_PORT } from './index';

describe('durable execution contracts', () => {
  it('exports local runtime defaults', () => {
    expect(DEFAULT_RUNTIME_HOST).toBe('127.0.0.1');
    expect(DEFAULT_RUNTIME_PORT).toBe(3477);
  });

  it('describes program identity and config', () => {
    const config: TrigoraConfig = {
      programs: './src/programs/**/*.ts',
      runtime: { port: DEFAULT_RUNTIME_PORT },
    };
    const program: ProgramIdentity = {
      id: 'researchAgent',
      exportName: 'researchAgent',
      file: 'src/programs/researchAgent.ts',
    };

    expect(config.programs).toBe('./src/programs/**/*.ts');
    expect(program.id).toBe(program.exportName);
  });

  it('describes execution records and wait conditions', () => {
    const wait: WaitCondition = { type: 'event', eventName: 'approved' };
    const execution: ExecutionRecord = {
      id: 'exec_local_1',
      programId: 'researchAgent',
      status: 'waiting',
      input: { query: 'durable agents' },
      wait,
      attempt: 1,
      createdAt: '2026-08-31T00:00:00.000Z',
      updatedAt: '2026-08-31T00:00:00.000Z',
    };

    expect(execution.wait?.type).toBe('event');
  });

  it('describes typed event definitions', () => {
    const approved: EventDefinition<{ reviewer: string }> = { name: 'approved' };
    expect(approved.name).toBe('approved');
  });

  it('describes compiler and runtime API envelopes', () => {
    const programs: ProgramIdentity[] = [
      { id: 'researchAgent', exportName: 'researchAgent', file: 'src/researchAgent.ts' },
    ];
    const compileRequest: CompileRequest = {
      sourceRoot: '/tmp/project',
      programs,
      files: [
        { path: 'src/researchAgent.ts', contents: 'export async function researchAgent() {}' },
      ],
    };
    const compileResult: CompileResult = {
      ok: true,
      artifact: { artifactHash: 'abc', compilerVersion: 'passthrough-local@0.9.0' },
      diagnostics: [],
    };
    const start: StartExecutionRequest = {
      programId: 'researchAgent',
      input: { query: 'durable agents' },
    };
    const listed: ListProgramsResponse = {
      artifact: compileResult.artifact,
      programs,
      runtime: { url: 'http://127.0.0.1:3477' },
    };

    expect(compileRequest.programs).toHaveLength(1);
    expect(compileResult.ok).toBe(true);
    expect(start.programId).toBe('researchAgent');
    expect(listed.runtime.url).toContain('3477');
  });
});
