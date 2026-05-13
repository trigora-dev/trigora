import type { WebhookFlowEvent } from '@trigora/contracts';
import {
  computeHmacSha256Hex,
  getHeaderCaseInsensitive,
  isHexString,
  requireWebhookRequestParts,
  timingSafeEqualStrings,
} from './webhookSignature';

export type VerifyGitHubWebhookOptions = {
  secret: string | undefined;
};

const GITHUB_SIGNATURE_HEADER = 'x-hub-signature-256';
const GITHUB_SIGNATURE_PREFIX = 'sha256=';

export class GitHubWebhookVerificationError extends Error {
  override name = 'GitHubWebhookVerificationError';
}

function createVerificationError(message: string): GitHubWebhookVerificationError {
  return new GitHubWebhookVerificationError(message);
}

function parseGitHubSignatureHeader(headerValue: string): string | null {
  const normalizedValue = headerValue.trim();

  if (!normalizedValue.toLowerCase().startsWith(GITHUB_SIGNATURE_PREFIX)) {
    return null;
  }

  const signature = normalizedValue.slice(GITHUB_SIGNATURE_PREFIX.length).trim().toLowerCase();

  if (!isHexString(signature)) {
    return null;
  }

  return signature;
}

export async function verifyGitHubWebhook<T = unknown>(
  event: WebhookFlowEvent,
  options: VerifyGitHubWebhookOptions,
): Promise<T> {
  if (!options.secret || options.secret.trim().length === 0) {
    throw createVerificationError('A GitHub webhook secret is required.');
  }

  let requestParts: { headers: Record<string, string>; rawBody: string };

  try {
    requestParts = requireWebhookRequestParts(event, 'GitHub');
  } catch (error) {
    throw createVerificationError(error instanceof Error ? error.message : String(error));
  }

  const signatureHeader = getHeaderCaseInsensitive(requestParts.headers, GITHUB_SIGNATURE_HEADER);

  if (!signatureHeader) {
    throw createVerificationError('X-Hub-Signature-256 header is required.');
  }

  const parsedSignature = parseGitHubSignatureHeader(signatureHeader);

  if (!parsedSignature) {
    throw createVerificationError('X-Hub-Signature-256 header is invalid.');
  }

  let expectedSignature: string;

  try {
    expectedSignature = await computeHmacSha256Hex(options.secret, requestParts.rawBody);
  } catch (error) {
    throw createVerificationError(error instanceof Error ? error.message : String(error));
  }

  if (!timingSafeEqualStrings(parsedSignature, expectedSignature)) {
    throw createVerificationError('GitHub webhook signature did not match.');
  }

  try {
    return JSON.parse(requestParts.rawBody) as T;
  } catch {
    throw createVerificationError('GitHub webhook body contained invalid JSON.');
  }
}
