import { describe, expect, it } from 'vitest';

import type {
  DeployProgramRequest,
  EventDefinition,
  Execution,
  ExecutionWait,
  ListProgramsResponse,
  ProgramSummary,
  Project,
  StartExecutionRequest,
  TrigoraConfig,
} from './index';
import { DEFAULT_PROJECT_SLUG, DEFAULT_RUNTIME_HOST, DEFAULT_RUNTIME_PORT } from './index';

describe('public contracts', () => {
  it('exports local runtime and default project constants', () => {
    expect(DEFAULT_RUNTIME_HOST).toBe('127.0.0.1');
    expect(DEFAULT_RUNTIME_PORT).toBe(3477);
    expect(DEFAULT_PROJECT_SLUG).toBe('default');
  });

  it('describes program config and public wait projections', () => {
    const config: TrigoraConfig = {
      programs: './src/programs/**/*.ts',
      runtime: { port: DEFAULT_RUNTIME_PORT },
    };
    const wait: ExecutionWait = { type: 'event', event: 'approved' };
    const execution: Execution = {
      id: 'exec_1',
      projectId: 'proj_default',
      programId: 'researchAgent',
      programName: 'researchAgent',
      programVersionId: 'ver_1',
      artifactHash: 'abc',
      engineFormatVersion: 1,
      status: 'waiting',
      input: { query: 'durable agents' },
      wait,
      attempt: 1,
      createdAt: '2026-08-31T00:00:00.000Z',
      updatedAt: '2026-08-31T00:00:00.000Z',
    };
    const listed: ListProgramsResponse = {
      programs: [
        {
          id: 'prog_1',
          name: 'researchAgent',
          language: 'typescript',
          currentVersionId: 'ver_1',
          updatedAt: execution.updatedAt,
        } satisfies ProgramSummary,
      ],
    };
    const start: StartExecutionRequest = {
      programId: 'researchAgent',
      input: { query: 'durable agents' },
    };
    const project: Project = {
      id: 'proj_default',
      workspaceId: 'ws_1',
      name: 'default',
      slug: DEFAULT_PROJECT_SLUG,
      createdAt: execution.createdAt,
    };
    const deploy: DeployProgramRequest = {
      name: 'researchAgent',
      artifact: {
        hash: 'abc',
        blob: '{}',
        engineFormatVersion: 1,
        languageSemanticsVersion: '1',
        frontendId: 'typescript',
        frontendVersion: '0.9.0',
      },
      effectBundle: { language: 'typescript', files: [] },
    };
    const approved: EventDefinition<{ reviewer: string }> = { name: 'approved' };

    expect(config.programs).toContain('programs');
    expect(execution.wait?.type).toBe('event');
    expect(listed.programs).toHaveLength(1);
    expect(start.programId).toBe('researchAgent');
    expect(project.slug).toBe('default');
    expect(deploy.artifact.blob).toBe('{}');
    expect(approved.name).toBe('approved');
  });
});
