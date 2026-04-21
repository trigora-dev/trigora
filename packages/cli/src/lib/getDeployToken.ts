export function getDeployToken(): string | undefined {
  const token = process.env.TRIGORA_DEPLOY_TOKEN?.trim();

  return token ? token : undefined;
}
