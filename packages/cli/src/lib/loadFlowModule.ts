import path from 'node:path';
import { pathToFileURL } from 'node:url';

import type { FlowDefinition } from '@trigora/contracts';
import { validateFlowModule } from './validateFlowModule';

export async function loadFlowModule(filePath: string): Promise<FlowDefinition> {
  const absolutePath = path.resolve(process.cwd(), filePath);
  const moduleUrl = pathToFileURL(absolutePath).href;
  const cacheBustedUrl = `${moduleUrl}?t=${Date.now()}`;

  let importedModule: Record<string, unknown>;

  try {
    importedModule = (await import(cacheBustedUrl)) as Record<string, unknown>;
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`Failed to import flow file "${filePath}": ${error.message}`);
    }

    throw new Error(`Failed to import flow file "${filePath}".`);
  }

  return validateFlowModule(filePath, importedModule.default);
}
