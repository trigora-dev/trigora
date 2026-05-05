import type { FlowEvent } from '@trigora/contracts';
import { describe, expect, it, vi } from 'vitest';
import {
  StripeWebhookVerificationError,
  verifyStripeWebhook,
  type VerifyStripeWebhookOptions,
} from './stripe';

type StripeTestPayload = {
  data?: {
    object?: {
      id: string;
    };
  };
  type?: string;
};

function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

async function createStripeSignature(
  secret: string,
  timestamp: number,
  rawBody: string,
): Promise<string> {
  const key = await crypto.subtle.importKey(
    'raw',
    toArrayBuffer(encodeUtf8(secret)),
    {
      name: 'HMAC',
      hash: 'SHA-256',
    },
    false,
    ['sign'],
  );
  const signature = await crypto.subtle.sign(
    'HMAC',
    key,
    toArrayBuffer(encodeUtf8(`${timestamp}.${rawBody}`)),
  );

  return bytesToHex(new Uint8Array(signature));
}

async function createStripeSignatureHeader(
  secret: string,
  timestamp: number,
  rawBody: string,
  extras: string[] = [],
): Promise<string> {
  const signature = await createStripeSignature(secret, timestamp, rawBody);
  return [`t=${timestamp}`, ...extras, `v1=${signature}`].join(',');
}

function createWebhookEvent(
  rawBody: string,
  signatureHeader?: string,
  headerName = 'stripe-signature',
): FlowEvent {
  const headers = signatureHeader ? { [headerName]: signatureHeader } : {};

  return {
    payload: {
      tampered: true,
    },
    request: {
      headers,
      method: 'POST',
      url: 'https://example.com/webhooks/stripe',
      rawBody,
    },
  };
}

async function verify(
  event: FlowEvent,
  options: VerifyStripeWebhookOptions,
): Promise<StripeTestPayload> {
  return verifyStripeWebhook<StripeTestPayload>(event, options);
}

describe('verifyStripeWebhook', () => {
  it('verifies a valid Stripe signature and returns the parsed raw body', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{"type":"checkout.session.completed","data":{"object":{"id":"cs_test_123"}}}';
    const header = await createStripeSignatureHeader(secret, timestamp, rawBody);

    vi.setSystemTime(new Date(timestamp * 1000));

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).resolves.toEqual({
      type: 'checkout.session.completed',
      data: {
        object: {
          id: 'cs_test_123',
        },
      },
    });

    vi.useRealTimers();
  });

  it('throws when the Stripe webhook secret is missing', async () => {
    await expect(
      verify(createWebhookEvent('{}', 't=1,v1=abc12345'), { secret: undefined }),
    ).rejects.toThrow(StripeWebhookVerificationError);
    await expect(
      verify(createWebhookEvent('{}', 't=1,v1=abc12345'), { secret: undefined }),
    ).rejects.toThrow('A Stripe webhook secret is required.');
  });

  it('throws when request metadata is missing', async () => {
    await expect(verify({ payload: {} }, { secret: 'whsec_test_secret' })).rejects.toThrow(
      'Stripe webhook request metadata is required.',
    );
  });

  it('throws when rawBody is missing', async () => {
    await expect(
      verify(
        {
          payload: {},
          request: {
            headers: {},
            method: 'POST',
            url: 'https://example.com/webhooks/stripe',
            // @ts-expect-error testing invalid runtime input
            rawBody: undefined,
          },
        },
        { secret: 'whsec_test_secret' },
      ),
    ).rejects.toThrow('Stripe webhook raw body is required.');
  });

  it('throws when the Stripe-Signature header is missing', async () => {
    await expect(
      verify(createWebhookEvent('{"type":"checkout.session.completed"}'), {
        secret: 'whsec_test_secret',
      }),
    ).rejects.toThrow('Stripe-Signature header is required.');
  });

  it('throws when the Stripe signature header is invalid', async () => {
    await expect(
      verify(
        createWebhookEvent('{"type":"checkout.session.completed"}', 't=not-a-timestamp,v1=abc'),
        { secret: 'whsec_test_secret' },
      ),
    ).rejects.toThrow('Stripe-Signature header is invalid.');
  });

  it('throws when the timestamp is outside the allowed tolerance', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{"type":"checkout.session.completed"}';
    const header = await createStripeSignatureHeader(secret, timestamp, rawBody);

    vi.setSystemTime(new Date((timestamp + 301) * 1000));

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).rejects.toThrow(
      'Stripe webhook timestamp is outside the allowed tolerance.',
    );

    vi.useRealTimers();
  });

  it('throws when the signature does not match', async () => {
    const timestamp = 1_700_000_000;
    const rawBody = '{"type":"checkout.session.completed"}';
    const header = await createStripeSignatureHeader('whsec_wrong_secret', timestamp, rawBody);

    vi.setSystemTime(new Date(timestamp * 1000));

    await expect(
      verify(createWebhookEvent(rawBody, header), { secret: 'whsec_test_secret' }),
    ).rejects.toThrow('Stripe webhook signature did not match.');

    vi.useRealTimers();
  });

  it('passes when one of multiple v1 signatures matches', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{"type":"checkout.session.completed"}';
    const validHeader = await createStripeSignatureHeader(secret, timestamp, rawBody, [
      'v1=deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    ]);

    vi.setSystemTime(new Date(timestamp * 1000));

    await expect(verify(createWebhookEvent(rawBody, validHeader), { secret })).resolves.toEqual({
      type: 'checkout.session.completed',
    });

    vi.useRealTimers();
  });

  it('looks up the Stripe signature header case-insensitively', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{"type":"checkout.session.completed"}';
    const header = await createStripeSignatureHeader(secret, timestamp, rawBody);

    vi.setSystemTime(new Date(timestamp * 1000));

    await expect(
      verify(createWebhookEvent(rawBody, header, 'Stripe-Signature'), { secret }),
    ).resolves.toEqual({
      type: 'checkout.session.completed',
    });

    vi.useRealTimers();
  });

  it('throws when the raw body contains invalid JSON after successful verification', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{not-json';
    const header = await createStripeSignatureHeader(secret, timestamp, rawBody);

    vi.setSystemTime(new Date(timestamp * 1000));

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).rejects.toThrow(
      'Stripe webhook body contained invalid JSON.',
    );

    vi.useRealTimers();
  });

  it('disables timestamp tolerance when toleranceSeconds is 0', async () => {
    const timestamp = 1_700_000_000;
    const secret = 'whsec_test_secret';
    const rawBody = '{"type":"checkout.session.completed"}';
    const header = await createStripeSignatureHeader(secret, timestamp, rawBody);

    vi.setSystemTime(new Date((timestamp + 86_400) * 1000));

    await expect(
      verify(createWebhookEvent(rawBody, header), { secret, toleranceSeconds: 0 }),
    ).resolves.toEqual({
      type: 'checkout.session.completed',
    });

    vi.useRealTimers();
  });
});
