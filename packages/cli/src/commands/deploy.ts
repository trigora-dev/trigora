import type { DeployProgramRequest } from '@trigora/contracts';

import type { DiscoveredProgram } from '../lib/localRuntime/discoverPrograms';
import { buildEffectBundle } from '../lib/localRuntime/effectBundle';
import { discoverPrograms } from '../lib/localRuntime/discoverPrograms';
import { loadProjectConfig } from '../lib/localRuntime/loadConfig';
import { CliDisplayError, printSuccessSummary } from '../lib/cliOutput';
import { requireCloudClient, withCloud } from '../lib/cloudRuntime';
import { toWhoAmIApiFailure, toWhoAmITokenFailure } from '../lib/whoamiOutput';
import { getApiToken } from '../lib/getApiToken';

function requireToken(): string {
  const token = getApiToken();
  if (!token) {
    throw toWhoAmITokenFailure();
  }
  return token;
}

function toProgramSlug(id: string): string {
  const slug = id
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/[^a-zA-Z0-9]+/g, '-')
    .toLowerCase()
    .replace(/^-+|-+$/g, '')
    .slice(0, 64);

  if (!/^[a-z][a-z0-9-]*$/.test(slug)) {
    throw new CliDisplayError({
      title: 'Invalid program name',
      details: [
        { label: 'Program', value: id },
        {
          label: 'Reason',
          value: 'Cloud program names must be lowercase slugs starting with a letter.',
        },
      ],
    });
  }

  return slug;
}

function artifactMetadata(program: DiscoveredProgram): DeployProgramRequest['artifact'] {
  const parsed = JSON.parse(program.artifactJson) as {
    envelope?: {
      artifact_hash?: string;
      engine_format_version?: number;
      language_semantics_version?: string;
      frontend_id?: string;
      frontend_version?: string;
    };
  };
  const envelope = parsed.envelope ?? {};

  return {
    hash: envelope.artifact_hash ?? program.artifactHash,
    blob: program.artifactJson,
    engineFormatVersion: envelope.engine_format_version ?? 1,
    languageSemanticsVersion: envelope.language_semantics_version ?? 'unknown',
    frontendId: envelope.frontend_id ?? program.language,
    frontendVersion: envelope.frontend_version ?? program.compilerVersion,
  };
}

export async function deployCommand(options: { program?: string } = {}): Promise<void> {
  requireToken();
  const config = await loadProjectConfig();
  const programs = await discoverPrograms({
    rootDir: config.rootDir,
    globs: config.programGlobs,
  });
  const selected = options.program
    ? programs.filter(
        (program) => program.id === options.program || toProgramSlug(program.id) === options.program,
      )
    : programs;

  if (selected.length === 0) {
    throw new CliDisplayError({
      title: 'Program not found',
      details: [{ label: 'Program', value: options.program ?? '' }],
    });
  }

  const client = requireCloudClient();
  const deployed: Array<{ name: string; hash: string; language: string }> = [];

  for (const program of selected) {
    const name = toProgramSlug(program.id);
    const effectBundle = await buildEffectBundle(program);
    const result = await withCloud(() =>
      client.deployProgram({
        name,
        artifact: artifactMetadata(program),
        effectBundle,
      }),
    ).catch((error) => {
      throw toWhoAmIApiFailure(error, 'Deploying program');
    });

    deployed.push({
      name: result.program.name,
      hash: result.version.artifactHash,
      language: result.version.language,
    });
  }

  printSuccessSummary(
    deployed.length === 1 ? 'Program deployed' : 'Programs deployed',
    deployed.map((item) => ({
      label: item.name,
      value: `${item.language} · ${item.hash.slice(0, 12)}`,
    })),
  );
}
