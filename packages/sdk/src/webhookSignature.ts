import type { FlowEvent } from '@trigora/contracts';

export function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

export function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function getHeaderCaseInsensitive(
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

export function isHexString(value: string): boolean {
  return value.length > 0 && value.length % 2 === 0 && /^[0-9a-f]+$/i.test(value);
}

export function timingSafeEqualStrings(left: string, right: string): boolean {
  const leftBytes = encodeUtf8(left);
  const rightBytes = encodeUtf8(right);
  const length = Math.max(leftBytes.length, rightBytes.length);
  let difference = leftBytes.length === rightBytes.length ? 0 : 1;

  for (let index = 0; index < length; index += 1) {
    difference |= (leftBytes[index] ?? 0) ^ (rightBytes[index] ?? 0);
  }

  return difference === 0;
}

export async function computeHmacSha256Hex(secret: string, signedPayload: string): Promise<string> {
  const subtle = globalThis.crypto?.subtle;

  if (!subtle) {
    throw new Error('Web Crypto API is unavailable in this runtime.');
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

export function requireWebhookRequestParts(
  event: FlowEvent,
  providerName: string,
): { headers: Record<string, string>; rawBody: string } {
  if (
    !event.request ||
    typeof event.request.headers !== 'object' ||
    event.request.headers === null
  ) {
    throw new Error(`${providerName} webhook request metadata is required.`);
  }

  if (typeof event.request.rawBody !== 'string') {
    throw new Error(`${providerName} webhook raw body is required.`);
  }

  return {
    headers: event.request.headers,
    rawBody: event.request.rawBody,
  };
}
