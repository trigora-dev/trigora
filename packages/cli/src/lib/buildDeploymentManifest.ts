import fs from 'node:fs/promises';
import path from 'node:path';

import type { DeploymentManifest, HostedTrigger, Trigger } from '@trigora/contracts';
import { loadFlowModule } from './loadFlowModule';

type DeployOptions = {
  filePath?: string;
};

type LoadedFlow = {
  entrypoint: string;
  id: string;
  trigger: Trigger;
};

type DeployableFlow = {
  entrypoint: string;
  id: string;
  trigger: HostedTrigger;
};

export function formatTrigger(trigger: Trigger): string {
  switch (trigger.type) {
    case 'manual':
      return 'manual';
    case 'webhook':
      return trigger.event ? `webhook:${trigger.event}` : 'webhook';
    case 'cron':
      return `cron:${trigger.cron}`;
  }
}

function isDeployableTrigger(trigger: Trigger): trigger is HostedTrigger {
  return trigger.type === 'webhook' || trigger.type === 'cron';
}

function validateDeployableFlow(flow: LoadedFlow): DeployableFlow {
  if (!isDeployableTrigger(flow.trigger)) {
    throw new Error(
      `Flow "${flow.id}" in "${flow.entrypoint}" uses unsupported trigger "${flow.trigger.type}". trigora deploy currently supports only webhook- and cron-triggered flows.`,
    );
  }

  return {
    entrypoint: flow.entrypoint,
    id: flow.id,
    trigger: flow.trigger,
  };
}

function createDeploymentManifest(flow: DeployableFlow): DeploymentManifest {
  return {
    version: 1,
    flow: {
      entrypoint: flow.entrypoint,
      id: flow.id,
      trigger: flow.trigger,
    },
  };
}

async function findFlowFiles(searchDir: string): Promise<string[]> {
  let entries;

  try {
    entries = await fs.readdir(searchDir, { encoding: 'utf8', withFileTypes: true });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return [];
    }

    throw error;
  }

  const flowFiles: string[] = [];

  for (const entry of entries) {
    const fullPath = path.join(searchDir, entry.name);

    if (entry.isDirectory()) {
      flowFiles.push(...(await findFlowFiles(fullPath)));
      continue;
    }

    if (!entry.isFile()) {
      continue;
    }

    if (entry.name.endsWith('.ts') || entry.name.endsWith('.js')) {
      flowFiles.push(fullPath);
    }
  }

  return flowFiles.sort((left, right) => left.localeCompare(right));
}

async function discoverFlowPaths(cwd: string): Promise<string[]> {
  const flowFiles = await findFlowFiles(path.join(cwd, 'flows'));

  if (flowFiles.length === 0) {
    throw new Error(
      'No flow files found in "flows". Create one with "trigora init" or pass a specific flow to "trigora deploy <flow>".',
    );
  }

  return flowFiles;
}

async function resolveDeploymentFlowPath(cwd: string): Promise<string> {
  const flowFiles = await discoverFlowPaths(cwd);

  if (flowFiles.length === 1) {
    const [flowPath] = flowFiles;

    if (!flowPath) {
      throw new Error('Could not resolve the selected flow.');
    }

    return flowPath;
  }

  const visiblePaths = flowFiles
    .map((filePath) => path.relative(cwd, filePath))
    .map((filePath) => `- ${filePath}`)
    .join('\n');

  throw new Error(
    `Multiple flows found. Pass a flow name or file path to deploy one flow.\n${visiblePaths}`,
  );
}

async function toEntrypoint(cwd: string, filePath: string): Promise<string> {
  const resolvedCwd = await fs.realpath(cwd).catch(() => path.resolve(cwd));
  const absoluteFilePath = path.resolve(cwd, filePath);
  const resolvedFilePath = await fs.realpath(absoluteFilePath).catch(() => absoluteFilePath);

  return path.relative(resolvedCwd, resolvedFilePath);
}

async function loadDeployableFlow(cwd: string, filePath: string): Promise<LoadedFlow> {
  const absoluteFilePath = path.resolve(cwd, filePath);
  const flow = await loadFlowModule(absoluteFilePath);
  const entrypoint = await toEntrypoint(cwd, filePath);

  return {
    entrypoint,
    id: flow.id,
    trigger: flow.trigger,
  };
}

export async function buildDeploymentManifest(options: DeployOptions): Promise<DeploymentManifest> {
  const cwd = process.cwd();
  const filePath = options.filePath ?? (await resolveDeploymentFlowPath(cwd));
  const flow = await loadDeployableFlow(cwd, filePath);
  const deployableFlow = validateDeployableFlow(flow);

  return createDeploymentManifest(deployableFlow);
}
