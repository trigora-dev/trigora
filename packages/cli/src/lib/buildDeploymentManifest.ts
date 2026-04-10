import fs from 'node:fs/promises';
import path from 'node:path';

import type { DeploymentManifest, Trigger, WebhookTrigger } from '@trigora/contracts';
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
  trigger: WebhookTrigger;
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

function isDeployableTrigger(trigger: Trigger): trigger is WebhookTrigger {
  return trigger.type === 'webhook';
}

function createRoutePath(flowId: string): string {
  return `/${flowId}`;
}

function validateDeployableFlows(flows: LoadedFlow[]): DeployableFlow[] {
  const flowIds = new Map<string, string>();
  const deployableFlows: DeployableFlow[] = [];

  for (const flow of flows) {
    if (!isDeployableTrigger(flow.trigger)) {
      throw new Error(
        `Flow "${flow.id}" in "${flow.entrypoint}" uses unsupported trigger "${flow.trigger.type}". trigora deploy currently supports only webhook-triggered flows.`,
      );
    }

    const previousPath = flowIds.get(flow.id);

    if (previousPath) {
      throw new Error(
        `Duplicate flow id "${flow.id}" found in "${previousPath}" and "${flow.entrypoint}". Flow ids must be unique for deployment.`,
      );
    }

    flowIds.set(flow.id, flow.entrypoint);
    deployableFlows.push({
      entrypoint: flow.entrypoint,
      id: flow.id,
      trigger: flow.trigger,
    });
  }

  return deployableFlows;
}

function createDeploymentManifest(flows: DeployableFlow[]): DeploymentManifest {
  return {
    version: 1,
    flows: flows.map((flow) => ({
      entrypoint: flow.entrypoint,
      routePath: createRoutePath(flow.id),
      id: flow.id,
      trigger: flow.trigger,
    })),
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
  const filePaths = options.filePath ? [options.filePath] : await discoverFlowPaths(cwd);
  const flows = await Promise.all(filePaths.map((filePath) => loadDeployableFlow(cwd, filePath)));
  const deployableFlows = validateDeployableFlows(flows);

  return createDeploymentManifest(deployableFlows);
}
