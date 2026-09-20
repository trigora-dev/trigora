import type { JsonValue } from '@trigora/contracts';
import fs from 'node:fs/promises';

export async function parseJsonValue(value: string): Promise<JsonValue> {
  const trimmed = value.trim();
  if (
    trimmed.startsWith('{') ||
    trimmed.startsWith('[') ||
    trimmed.startsWith('"') ||
    trimmed === 'null' ||
    trimmed === 'true' ||
    trimmed === 'false' ||
    /^-?\d/.test(trimmed)
  ) {
    try {
      return JSON.parse(trimmed) as JsonValue;
    } catch {
      throw new Error(`Invalid JSON: ${value}`);
    }
  }

  return loadJsonFile(trimmed);
}

export async function loadJsonFile(filePath: string): Promise<JsonValue> {
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
