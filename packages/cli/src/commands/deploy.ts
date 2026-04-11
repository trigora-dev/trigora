import type { DeploymentManifest } from '@trigora/contracts';
import { colors } from '../lib/colors';
import { buildDeploymentArtifact } from '../lib/buildDeploymentArtifact';
import { buildDeploymentManifest, formatTrigger } from '../lib/buildDeploymentManifest';
import { createDeployApiClient } from '../lib/createDeployApiClient';

type DeployOptions = {
  filePath?: string;
};

function getDeployToken(): string {
  const token = process.env.TRIGORA_DEPLOY_TOKEN?.trim();

  if (!token) {
    throw new Error(
      'Missing deploy API configuration: TRIGORA_DEPLOY_TOKEN. Set this environment variable before running "trigora deploy".',
    );
  }

  return token;
}

export async function deployCommand(options: DeployOptions): Promise<DeploymentManifest> {
  const deployPrefix = colors.dev('[deploy]');
  console.log(`${deployPrefix} validating flow modules for deployment...`);

  const manifest = await buildDeploymentManifest(options);
  const artifact = await buildDeploymentArtifact(manifest);
  const apiClient = createDeployApiClient({
    token: getDeployToken(),
  });

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
  console.log(`${deployPrefix} sending deployment package to Trigora Cloud...`);

  const deployment = await apiClient.createDeployment({ manifest, artifact });

  console.log(
    `${deployPrefix} ${colors.success(`deployment ${deployment.id} ${deployment.status}`)}`,
  );

  if (deployment.baseUrl) {
    console.log(`${deployPrefix} base URL: ${deployment.baseUrl}`);
  }

  console.log(
    `${deployPrefix} validated deployable webhook flows and sent the deployment package to Trigora Cloud`,
  );

  return manifest;
}
