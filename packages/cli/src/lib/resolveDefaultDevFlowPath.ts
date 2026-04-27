import fs from 'node:fs/promises';
import path from 'node:path';

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

export async function resolveDefaultDevFlowPath(cwd = process.cwd()): Promise<string> {
  const flowFiles = await findFlowFiles(path.join(cwd, 'flows'));

  if (flowFiles.length === 0) {
    throw new Error(
      'No flow files found in "flows". Create one with "trigora init" or pass a specific flow to "trigora dev <flow>".',
    );
  }

  if (flowFiles.length > 1) {
    throw new Error(
      'Multiple flow files found in "flows". Pass a specific flow to "trigora dev <flow>".',
    );
  }

  return flowFiles[0] as string;
}
