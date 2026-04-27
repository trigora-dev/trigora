import fs from 'node:fs/promises';

export async function loadJsonFile(filePath: string): Promise<unknown> {
  let raw: string;

  try {
    raw = await fs.readFile(filePath, 'utf-8');
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`Failed to read payload file "${filePath}": ${error.message}`);
    }

    throw new Error(`Failed to read payload file "${filePath}".`);
  }

  try {
    return JSON.parse(raw);
  } catch {
    throw new Error(`Invalid JSON in payload file "${filePath}".`);
  }
}
