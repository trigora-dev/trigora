import type { DeploymentManifest } from '@trigora/contracts';
import { colors } from '../lib/colors';
import { buildDeploymentArtifact } from '../lib/buildDeploymentArtifact';
import { buildDeploymentManifest, formatTrigger } from '../lib/buildDeploymentManifest';

type DeployOptions = {
  filePath?: string;
};

export async function deployCommand(options: DeployOptions): Promise<DeploymentManifest> {
  const deployPrefix = colors.dev('[deploy]');
  console.log(`${deployPrefix} validating flow modules for deployment...`);

  const manifest = await buildDeploymentManifest(options);
  const artifact = await buildDeploymentArtifact(manifest);

  console.log(
    `${deployPrefix} ${colors.flow(`prepared ${manifest.flows.length} flow(s) for deployment`)}`,
  );

  for (const flow of manifest.flows) {
    const triggerLabel = formatTrigger(flow.trigger);

    console.log(
      `${deployPrefix} ${colors.flow(flow.id)} ${colors.warn(`(${triggerLabel})`)} ${flow.entrypoint} ${colors.dev(`→ ${flow.routePath}`)}`,
    );
  }

  console.log('');
  console.log(`${deployPrefix} ${colors.flow('deployment manifest')}`);

  for (const flow of manifest.flows) {
    console.log(`${deployPrefix} ${flow.entrypoint} ${colors.dev(`→ route ${flow.routePath}`)}`);
  }

  console.log('');
  console.log(
    `${deployPrefix} ${colors.flow(`built deployment artifact with ${artifact.files.length} file(s)`)}`,
  );
  console.log(
    `${deployPrefix} ${colors.warn('backend deployment API is not wired into this CLI yet')}`,
  );
  console.log(
    `${deployPrefix} validated deployable webhook flows and assembled a deployment package locally`,
  );

  return manifest;
}
