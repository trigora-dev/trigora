import path from 'node:path';
import { pathToFileURL } from 'node:url';

import type { FlowDefinition } from '@trigora/contracts';

export async function loadFlowModule(filePath: string): Promise<FlowDefinition> {
  const absolutePath = path.resolve(process.cwd(), filePath);
  const moduleUrl = pathToFileURL(absolutePath).href;
  const cacheBustedUrl = `${moduleUrl}?t=${Date.now()}`;

  const importedModule = await import(cacheBustedUrl);
  const flow = importedModule.default;

  if (!flow) {
    throw new Error(`No default export found in "${filePath}". Expected a default exported flow.`);
  }

  if (typeof flow !== 'object') {
    throw new Error(`Default export in "${filePath}" is not a valid flow object.`);
  }

  if (!('id' in flow) || !('trigger' in flow) || !('run' in flow)) {
    throw new Error(
      `Default export in "${filePath}" is missing required flow fields: id, trigger, run.`,
    );
  }

  return flow as FlowDefinition;
}
