import type { FlowEvent } from '@trigora/contracts';
import {
  computeHmacSha256Hex,
  getHeaderCaseInsensitive,
  isHexString,
  requireWebhookRequestParts,
  timingSafeEqualStrings,
} from './webhookSignature';

export type VerifyStripeWebhookOptions = {
  secret: string | undefined;
  toleranceSeconds?: number;
};

const DEFAULT_TOLERANCE_SECONDS = 300;
const STRIPE_SIGNATURE_HEADER = 'stripe-signature';

export class StripeWebhookVerificationError extends Error {
  override name = 'StripeWebhookVerificationError';
}

function createVerificationError(message: string): StripeWebhookVerificationError {
  return new StripeWebhookVerificationError(message);
}

function parseStripeSignatureHeader(
  headerValue: string,
): { timestamp: number; signatures: string[] } | null {
  let timestamp: number | undefined;
  const signatures: string[] = [];

  for (const part of headerValue.split(',')) {
    const separatorIndex = part.indexOf('=');

    if (separatorIndex <= 0) {
      continue;
    }

    const key = part.slice(0, separatorIndex).trim();
    const value = part.slice(separatorIndex + 1).trim();

    if (key === 't') {
      const parsedTimestamp = Number(value);

      if (!Number.isInteger(parsedTimestamp)) {
        return null;
      }

      timestamp = parsedTimestamp;
      continue;
    }

    if (key === 'v1' && isHexString(value)) {
      signatures.push(value.toLowerCase());
    }
  }

  if (timestamp === undefined || signatures.length === 0) {
    return null;
  }

  return { timestamp, signatures };
}

export async function verifyStripeWebhook<T = unknown>(
  event: FlowEvent,
  options: VerifyStripeWebhookOptions,
): Promise<T> {
  if (!options.secret || options.secret.trim().length === 0) {
    throw createVerificationError('A Stripe webhook secret is required.');
  }

  let requestParts: { headers: Record<string, string>; rawBody: string };

  try {
    requestParts = requireWebhookRequestParts(event, 'Stripe');
  } catch (error) {
    throw createVerificationError(error instanceof Error ? error.message : String(error));
  }

  const signatureHeader = getHeaderCaseInsensitive(requestParts.headers, STRIPE_SIGNATURE_HEADER);

  if (!signatureHeader) {
    throw createVerificationError('Stripe-Signature header is required.');
  }

  const parsedHeader = parseStripeSignatureHeader(signatureHeader);

  if (!parsedHeader) {
    throw createVerificationError('Stripe-Signature header is invalid.');
  }

  const toleranceSeconds = options.toleranceSeconds ?? DEFAULT_TOLERANCE_SECONDS;

  if (toleranceSeconds !== 0) {
    const currentTimestamp = Math.floor(Date.now() / 1000);

    if (Math.abs(currentTimestamp - parsedHeader.timestamp) > toleranceSeconds) {
      throw createVerificationError('Stripe webhook timestamp is outside the allowed tolerance.');
    }
  }

  const signedPayload = `${parsedHeader.timestamp}.${requestParts.rawBody}`;
  let expectedSignature: string;

  try {
    expectedSignature = await computeHmacSha256Hex(options.secret, signedPayload);
  } catch (error) {
    throw createVerificationError(error instanceof Error ? error.message : String(error));
  }
  const matches = parsedHeader.signatures.some((signature) =>
    timingSafeEqualStrings(signature, expectedSignature),
  );

  if (!matches) {
    throw createVerificationError('Stripe webhook signature did not match.');
  }

  try {
    return JSON.parse(requestParts.rawBody) as T;
  } catch {
    throw createVerificationError('Stripe webhook body contained invalid JSON.');
  }
}
