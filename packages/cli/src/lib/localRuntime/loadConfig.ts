import fs from 'node:fs/promises';
import path from 'node:path';

import {
  DEFAULT_RUNTIME_HOST,
  DEFAULT_RUNTIME_PORT,
  type ResolvedTrigoraConfig,
  type TriggerConfig,
} from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';
import { parseSchemaManifest } from './manifest';

const TOML_CONFIG = 'trigora.toml';

export type ParsedTrigoraToml = {
  projectName: string;
  programs: string[];
  triggers: TriggerConfig[];
};

async function pathExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

export async function findConfigPath(cwd = process.cwd()): Promise<string | undefined> {
  const tomlPath = path.join(cwd, TOML_CONFIG);
  if (await pathExists(tomlPath)) {
    return tomlPath;
  }

  return undefined;
}

export function parseTrigoraToml(source: string): ParsedTrigoraToml {
  const manifest = parseSchemaManifest(source);
  return {
    projectName: manifest.projectName,
    programs: manifest.programs,
    triggers: manifest.triggers,
  };
}

export function resolveConfig(
  config: ParsedTrigoraToml,
  options: { configPath: string; rootDir: string },
): ResolvedTrigoraConfig {
  return {
    configPath: options.configPath,
    rootDir: options.rootDir,
    programGlobs: config.programs,
    projectName: config.projectName,
    triggers: config.triggers,
    runtime: {
      host: DEFAULT_RUNTIME_HOST,
      port: DEFAULT_RUNTIME_PORT,
    },
    compiler: {},
  };
}

export async function loadProjectConfig(cwd = process.cwd()): Promise<ResolvedTrigoraConfig> {
  const configPath = await findConfigPath(cwd);

  if (!configPath) {
    throw new CliDisplayError({
      title: 'No trigora.toml found',
      details: [{ label: 'Looked in', value: cwd }],
      hint: 'Run `trigora init` or add trigora.toml with `[project].programs`.',
    });
  }

  let loaded: ParsedTrigoraToml;
  try {
    loaded = parseTrigoraToml(await fs.readFile(configPath, 'utf8'));
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new CliDisplayError({
      title: 'Invalid trigora.toml',
      details: [
        { label: 'File', value: path.relative(cwd, configPath) },
        { label: 'Reason', value: reason },
      ],
      hint: 'Example: programs = ["src/**/*.ts"] under [project].',
    });
  }

  return resolveConfig(loaded, { configPath, rootDir: cwd });
}
