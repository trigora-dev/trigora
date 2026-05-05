import type { FlowEvent } from '@trigora/contracts';

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

function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function getHeaderCaseInsensitive(
  headers: Record<string, string>,
  headerName: string,
): string | undefined {
  const normalizedName = headerName.toLowerCase();

  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === normalizedName) {
      return value;
    }
  }

  return undefined;
}

function isHexString(value: string): boolean {
  return value.length > 0 && value.length % 2 === 0 && /^[0-9a-f]+$/i.test(value);
}

function timingSafeEqualStrings(left: string, right: string): boolean {
  const leftBytes = encodeUtf8(left);
  const rightBytes = encodeUtf8(right);
  const length = Math.max(leftBytes.length, rightBytes.length);
  let difference = leftBytes.length === rightBytes.length ? 0 : 1;

  for (let index = 0; index < length; index += 1) {
    difference |= (leftBytes[index] ?? 0) ^ (rightBytes[index] ?? 0);
  }

  return difference === 0;
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

async function computeHmacSha256Hex(secret: string, signedPayload: string): Promise<string> {
  const subtle = globalThis.crypto?.subtle;

  if (!subtle) {
    throw createVerificationError('Web Crypto API is unavailable in this runtime.');
  }

  const key = await subtle.importKey(
    'raw',
    toArrayBuffer(encodeUtf8(secret)),
    {
      name: 'HMAC',
      hash: 'SHA-256',
    },
    false,
    ['sign'],
  );
  const signature = await subtle.sign('HMAC', key, toArrayBuffer(encodeUtf8(signedPayload)));

  return bytesToHex(new Uint8Array(signature));
}

export async function verifyStripeWebhook<T = unknown>(
  event: FlowEvent,
  options: VerifyStripeWebhookOptions,
): Promise<T> {
  if (!options.secret || options.secret.trim().length === 0) {
    throw createVerificationError('A Stripe webhook secret is required.');
  }

  if (
    !event.request ||
    typeof event.request.headers !== 'object' ||
    event.request.headers === null
  ) {
    throw createVerificationError('Stripe webhook request metadata is required.');
  }

  if (typeof event.request.rawBody !== 'string') {
    throw createVerificationError('Stripe webhook raw body is required.');
  }

  const signatureHeader = getHeaderCaseInsensitive(event.request.headers, STRIPE_SIGNATURE_HEADER);

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

  const signedPayload = `${parsedHeader.timestamp}.${event.request.rawBody}`;
  const expectedSignature = await computeHmacSha256Hex(options.secret, signedPayload);
  const matches = parsedHeader.signatures.some((signature) =>
    timingSafeEqualStrings(signature, expectedSignature),
  );

  if (!matches) {
    throw createVerificationError('Stripe webhook signature did not match.');
  }

  try {
    return JSON.parse(event.request.rawBody) as T;
  } catch {
    throw createVerificationError('Stripe webhook body contained invalid JSON.');
  }
}
