import fs from 'node:fs';
import path from 'node:path';

import type { ArtifactIdentity, ProgramIdentity, ResolvedTrigoraConfig } from '@trigora/contracts';
import { colors } from '../lib/colors';
import { printSuccessSummary } from '../lib/cliOutput';
import { compilePrograms, readCompileFiles } from '../lib/localRuntime/compiler';
import {
  discoverPrograms,
  toProgramIdentity,
  type DiscoveredProgram,
} from '../lib/localRuntime/discoverPrograms';
import { LocalExecutionEngine } from '../lib/localRuntime/engine';
import { loadProjectConfig } from '../lib/localRuntime/loadConfig';
import { startLocalRuntimeServer } from '../lib/localRuntime/server';

export type DevCommandOptions = {
  host?: string;
  port?: number;
};

const FILE_CHANGE_DEBOUNCE_MS = 100;

function formatProgramList(programs: ProgramIdentity[]): string {
  return programs.map((program) => program.id).join(', ');
}

function printEngineEvent(event: {
  type: string;
  execution: {
    id: string;
    programId: string;
    status: string;
    wait?: { type: string; eventName?: string };
  };
  name?: string;
}): void {
  const executionId = colors.flow(event.execution.id);
  const program = colors.heading(event.execution.programId);

  if (event.type === 'started') {
    console.log(`${colors.info('▶')} Started ${program} ${executionId}`);
    return;
  }

  if (event.type === 'waiting') {
    const wait =
      event.execution.wait?.type === 'event'
        ? `event ${colors.heading(event.execution.wait.eventName ?? '')}`
        : event.execution.wait?.type === 'timer'
          ? 'timer'
          : event.execution.wait?.type === 'child'
            ? 'child execution'
            : 'durable boundary';
    console.log(`${colors.warn('⏸')} ${program} ${executionId} waiting on ${wait}`);
    return;
  }

  if (event.type === 'resumed') {
    console.log(`${colors.info('▶')} ${program} ${executionId} resumed`);
    return;
  }

  if (event.type === 'completed') {
    console.log(`${colors.success('✔')} ${program} ${executionId} completed`);
    return;
  }

  if (event.type === 'failed') {
    console.log(`${colors.error('✖')} ${program} ${executionId} failed`);
    return;
  }

  if (event.type === 'cancelled') {
    console.log(`${colors.warn('■')} ${program} ${executionId} cancelled`);
    return;
  }

  if (event.type === 'effect' && event.name) {
    console.log(
      `${colors.dev('·')} ${program} ${executionId} effect ${colors.heading(event.name)}`,
    );
  }
}

async function loadWorkspace(config: ResolvedTrigoraConfig): Promise<{
  artifact: ArtifactIdentity;
  programs: DiscoveredProgram[];
}> {
  const programs = await discoverPrograms({
    rootDir: config.rootDir,
    globs: config.programGlobs,
  });
  const files = await readCompileFiles(config.rootDir, programs.map(toProgramIdentity));
  const compiled = await compilePrograms(
    {
      sourceRoot: config.rootDir,
      programs: programs.map(toProgramIdentity),
      files,
    },
    config.compiler.endpoint,
  );

  return {
    artifact: compiled.artifact,
    programs,
  };
}

export async function devCommand(options: DevCommandOptions = {}): Promise<void> {
  const config = await loadProjectConfig();
  const host = options.host?.trim() || config.runtime.host;
  const requestedPort = options.port ?? config.runtime.port;
  let workspace = await loadWorkspace(config);
  const engine = new LocalExecutionEngine((event) => {
    printEngineEvent(event);
  });
  engine.replacePrograms(workspace.programs);

  const server = await startLocalRuntimeServer({
    artifact: workspace.artifact,
    engine,
    host,
    port: requestedPort,
  });

  if (server.port !== requestedPort) {
    console.log(colors.warn(`Port ${requestedPort} was in use, using ${server.port} instead.`));
  }

  printSuccessSummary(
    'Local runtime ready',
    [
      { label: 'Programs', value: formatProgramList(workspace.programs) },
      { label: 'Runtime', value: colors.link(server.url) },
      { label: 'Artifact', value: workspace.artifact.artifactHash.slice(0, 12) },
      { label: 'Compiler', value: workspace.artifact.compilerVersion },
    ],
    [],
    'Start executions with `@trigora/client` while this process is running.',
  );

  const watchers: fs.FSWatcher[] = [];
  const debounceTimers = new Map<string, NodeJS.Timeout>();
  let shuttingDown = false;

  function watchedPaths(): string[] {
    return [
      config.configPath,
      ...workspace.programs.map((program) => path.join(config.rootDir, program.file)),
    ];
  }

  async function reload(label: string): Promise<void> {
    if (shuttingDown) {
      return;
    }

    try {
      workspace = await loadWorkspace(config);
      engine.replacePrograms(workspace.programs);
      console.log('');
      console.log(
        colors.label(`${label} changed. Reloaded ${formatProgramList(workspace.programs)}.`),
      );
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      console.error('');
      console.error(`${colors.error('✖')} Reload failed`);
      console.error(reason);
    }
  }

  function watch(filePath: string, label: string): void {
    try {
      const watcher = fs.watch(filePath, (eventType) => {
        if (eventType !== 'change' || shuttingDown) {
          return;
        }

        const existing = debounceTimers.get(filePath);
        if (existing) {
          clearTimeout(existing);
        }

        debounceTimers.set(
          filePath,
          setTimeout(() => {
            debounceTimers.delete(filePath);
            void reload(label);
          }, FILE_CHANGE_DEBOUNCE_MS),
        );
      });
      watchers.push(watcher);
    } catch {
      // File may disappear during reload; ignore.
    }
  }

  function shutdown(): void {
    if (shuttingDown) {
      return;
    }

    shuttingDown = true;
    for (const watcher of watchers) {
      watcher.close();
    }
    for (const timer of debounceTimers.values()) {
      clearTimeout(timer);
    }
    process.off('SIGINT', shutdown);
    process.off('SIGTERM', shutdown);
    void server.close().finally(() => {
      console.log('');
      console.log(colors.label('Stopped'));
      process.exit(0);
    });
  }

  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);

  for (const filePath of watchedPaths()) {
    watch(filePath, path.basename(filePath));
  }

  await new Promise<void>(() => {});
}
