import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MISSING = 'The local runtime binary is missing. Reinstall trigora.';

export function resolveLocalBinary(): string {
  const override = process.env.TRIGORA_LOCAL_BIN?.trim();
  if (override) {
    if (!fs.existsSync(override)) {
      throw new Error(`TRIGORA_LOCAL_BIN does not exist: ${override}`);
    }
    return override;
  }

  const name = process.platform === 'win32' ? 'trigora-local.exe' : 'trigora-local';
  const binary = path.join(
    packageRoot(),
    'vendor',
    'trigora-local',
    `${process.platform}-${process.arch}`,
    name,
  );
  if (!fs.existsSync(binary)) {
    throw new Error(MISSING);
  }
  return binary;
}

function packageRoot(): string {
  let dir = path.dirname(fileURLToPath(import.meta.url));
  for (let attempt = 0; attempt < 8; attempt += 1) {
    const manifest = path.join(dir, 'package.json');
    if (fs.existsSync(manifest)) {
      try {
        const parsed = JSON.parse(fs.readFileSync(manifest, 'utf8')) as { name?: string };
        if (parsed.name === 'trigora') {
          return dir;
        }
      } catch {
        // Keep walking until the CLI package manifest is found.
      }
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      break;
    }
    dir = parent;
  }
  throw new Error(MISSING);
}
