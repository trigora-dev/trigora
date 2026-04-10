import path from 'node:path';

import { build } from 'esbuild';
import type { DeploymentArtifact, DeploymentManifest } from '@trigora/contracts';

function toBundlePath(entrypoint: string): string {
  const extension = path.extname(entrypoint);
  return `${entrypoint.slice(0, -extension.length)}.mjs`;
}

export async function buildDeploymentArtifact(
  manifest: DeploymentManifest,
): Promise<DeploymentArtifact> {
  const cwd = process.cwd();

  const files = await Promise.all(
    manifest.flows.map(async (flow) => {
      try {
        const result = await build({
          absWorkingDir: cwd,
          bundle: true,
          entryPoints: [flow.entrypoint],
          format: 'esm',
          logLevel: 'silent',
          platform: 'node',
          target: 'node20',
          write: false,
        });

        const outputFile = result.outputFiles[0];

        if (!outputFile) {
          throw new Error(`No bundled output was produced for "${flow.entrypoint}".`);
        }

        return {
          entrypoint: flow.entrypoint,
          path: toBundlePath(flow.entrypoint),
          contents: outputFile.text,
        };
      } catch (error) {
        if (error instanceof Error) {
          throw new Error(
            `Failed to bundle flow "${flow.entrypoint}" for deployment: ${error.message}`,
          );
        }

        throw new Error(`Failed to bundle flow "${flow.entrypoint}" for deployment.`);
      }
    }),
  );

  return {
    version: 1,
    format: 'esm',
    target: 'node20',
    files,
  };
}
