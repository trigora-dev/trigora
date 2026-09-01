import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import { build } from 'esbuild';
import {
  DEFAULT_RUNTIME_HOST,
  DEFAULT_RUNTIME_PORT,
  type ResolvedTrigoraConfig,
  type TrigoraConfig,
} from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';

const CONFIG_FILENAMES = [
  'trigora.config.ts',
  'trigora.config.mts',
  'trigora.config.js',
  'trigora.config.mjs',
];

function isTrigoraConfig(value: unknown): value is TrigoraConfig {
  if (typeof value !== 'object' || value === null) {
    return false;
  }

  const programs = (value as TrigoraConfig).programs;
  return typeof programs === 'string' || Array.isArray(programs);
}

async function pathExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

export async function findConfigPath(cwd = process.cwd()): Promise<string | undefined> {
  for (const filename of CONFIG_FILENAMES) {
    const candidate = path.join(cwd, filename);
    if (await pathExists(candidate)) {
      return candidate;
    }
  }

  return undefined;
}

async function importBundledConfig(configPath: string): Promise<unknown> {
  const outfile = path.join(
    os.tmpdir(),
    `trigora-config-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`,
  );

  try {
    await build({
      absWorkingDir: path.dirname(configPath),
      bundle: true,
      entryPoints: [configPath],
      format: 'esm',
      logLevel: 'silent',
      outfile,
      packages: 'external',
      platform: 'node',
      target: 'node20',
    });

    const imported = (await import(pathToFileURL(outfile).href)) as { default?: unknown };
    return imported.default ?? imported;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new CliDisplayError({
      title: 'Failed to load trigora.config',
      details: [
        { label: 'File', value: path.relative(process.cwd(), configPath) || configPath },
        { label: 'Reason', value: reason },
      ],
    });
  } finally {
    await fs.rm(outfile, { force: true }).catch(() => undefined);
  }
}

export function resolveConfig(
  config: TrigoraConfig,
  options: { configPath: string; rootDir: string },
): ResolvedTrigoraConfig {
  const programGlobs = (Array.isArray(config.programs) ? config.programs : [config.programs])
    .map((pattern) => pattern.trim())
    .filter(Boolean);

  if (programGlobs.length === 0) {
    throw new CliDisplayError({
      title: 'Invalid trigora.config',
      details: [
        { label: 'File', value: path.relative(options.rootDir, options.configPath) },
        { label: 'Reason', value: '`programs` must be one or more entrypoint globs.' },
      ],
      hint: 'Example: defineConfig({ programs: "./src/programs/**/*.ts" })',
    });
  }

  return {
    configPath: options.configPath,
    rootDir: options.rootDir,
    programGlobs,
    runtime: {
      host: config.runtime?.host?.trim() || DEFAULT_RUNTIME_HOST,
      port: config.runtime?.port ?? DEFAULT_RUNTIME_PORT,
    },
    compiler: {
      endpoint: config.compiler?.endpoint?.trim() || undefined,
    },
  };
}

export async function loadProjectConfig(cwd = process.cwd()): Promise<ResolvedTrigoraConfig> {
  const configPath = await findConfigPath(cwd);

  if (!configPath) {
    throw new CliDisplayError({
      title: 'No trigora.config found',
      details: [{ label: 'Looked in', value: cwd }],
      hint: 'Run `trigora init` or add a trigora.config.ts with a `programs` glob.',
    });
  }

  const loaded = await importBundledConfig(configPath);

  if (!isTrigoraConfig(loaded)) {
    throw new CliDisplayError({
      title: 'Invalid trigora.config',
      details: [
        { label: 'File', value: path.relative(cwd, configPath) },
        { label: 'Reason', value: 'Default export must be a config object with `programs`.' },
      ],
      hint: 'Use defineConfig({ programs: "./src/programs/**/*.ts" }) from @trigora/sdk.',
    });
  }

  return resolveConfig(loaded, {
    configPath,
    rootDir: cwd,
  });
}
