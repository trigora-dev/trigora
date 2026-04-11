import fs from 'node:fs';
import path from 'node:path';
import dotenv from 'dotenv';

function readEnvFile(filePath: string): Record<string, string> {
  let contents: string;

  try {
    contents = fs.readFileSync(filePath, 'utf-8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return {};
    }

    throw error;
  }

  return dotenv.parse(contents);
}

export function loadProjectEnv(cwd = process.cwd()): void {
  const originalKeys = new Set(Object.keys(process.env));
  const envFilePaths = [path.join(cwd, '.env'), path.join(cwd, '.env.local')];

  for (const filePath of envFilePaths) {
    const values = readEnvFile(filePath);

    for (const [key, value] of Object.entries(values)) {
      if (originalKeys.has(key)) {
        continue;
      }

      process.env[key] = value;
    }
  }
}
