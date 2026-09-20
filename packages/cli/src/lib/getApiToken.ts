export function getApiToken(): string | undefined {
  const token = process.env.TRIGORA_TOKEN?.trim();
  return token ? token : undefined;
}
