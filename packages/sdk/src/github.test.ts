import type { FlowEvent } from '@trigora/contracts';
import { describe, expect, it } from 'vitest';
import {
  GitHubWebhookVerificationError,
  verifyGitHubWebhook,
  type VerifyGitHubWebhookOptions,
} from './github';
import { computeHmacSha256Hex } from './webhookSignature';

type GitHubTestPayload = {
  action?: string;
  repository?: {
    full_name?: string;
  };
};

async function createGitHubSignatureHeader(secret: string, rawBody: string): Promise<string> {
  const signature = await computeHmacSha256Hex(secret, rawBody);
  return `sha256=${signature}`;
}

function createWebhookEvent(
  rawBody: string,
  signatureHeader?: string,
  headerName = 'x-hub-signature-256',
): FlowEvent {
  const headers = signatureHeader ? { [headerName]: signatureHeader } : {};

  return {
    payload: {
      tampered: true,
    },
    request: {
      headers,
      method: 'POST',
      url: 'https://example.com/webhooks/github',
      rawBody,
    },
  };
}

async function verify(
  event: FlowEvent,
  options: VerifyGitHubWebhookOptions,
): Promise<GitHubTestPayload> {
  return verifyGitHubWebhook<GitHubTestPayload>(event, options);
}

describe('verifyGitHubWebhook', () => {
  it('verifies a valid GitHub signature and returns the parsed raw body', async () => {
    const secret = 'github_test_secret';
    const rawBody = '{"action":"opened","repository":{"full_name":"trigora-dev/trigora"}}';
    const header = await createGitHubSignatureHeader(secret, rawBody);

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).resolves.toEqual({
      action: 'opened',
      repository: {
        full_name: 'trigora-dev/trigora',
      },
    });
  });

  it('throws when the GitHub webhook secret is missing', async () => {
    await expect(
      verify(createWebhookEvent('{}', 'sha256=abc12345'), { secret: undefined }),
    ).rejects.toThrow(GitHubWebhookVerificationError);
    await expect(
      verify(createWebhookEvent('{}', 'sha256=abc12345'), { secret: undefined }),
    ).rejects.toThrow('A GitHub webhook secret is required.');
  });

  it('throws when request metadata is missing', async () => {
    await expect(verify({ payload: {} }, { secret: 'github_test_secret' })).rejects.toThrow(
      'GitHub webhook request metadata is required.',
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
            url: 'https://example.com/webhooks/github',
            // @ts-expect-error testing invalid runtime input
            rawBody: undefined,
          },
        },
        { secret: 'github_test_secret' },
      ),
    ).rejects.toThrow('GitHub webhook raw body is required.');
  });

  it('throws when the GitHub signature header is missing', async () => {
    await expect(
      verify(createWebhookEvent('{"action":"opened"}'), {
        secret: 'github_test_secret',
      }),
    ).rejects.toThrow('X-Hub-Signature-256 header is required.');
  });

  it('throws when the GitHub signature header is invalid', async () => {
    await expect(
      verify(createWebhookEvent('{"action":"opened"}', 'sha512=abc12345'), {
        secret: 'github_test_secret',
      }),
    ).rejects.toThrow('X-Hub-Signature-256 header is invalid.');
  });

  it('throws when the signature does not match', async () => {
    const rawBody = '{"action":"opened"}';
    const header = await createGitHubSignatureHeader('github_wrong_secret', rawBody);

    await expect(
      verify(createWebhookEvent(rawBody, header), { secret: 'github_test_secret' }),
    ).rejects.toThrow('GitHub webhook signature did not match.');
  });

  it('looks up the GitHub signature header case-insensitively', async () => {
    const secret = 'github_test_secret';
    const rawBody = '{"action":"opened"}';
    const header = await createGitHubSignatureHeader(secret, rawBody);

    await expect(
      verify(createWebhookEvent(rawBody, header, 'X-Hub-Signature-256'), { secret }),
    ).resolves.toEqual({
      action: 'opened',
    });
  });

  it('accepts uppercase hex signatures', async () => {
    const secret = 'github_test_secret';
    const rawBody = '{"action":"opened"}';
    const header = (await createGitHubSignatureHeader(secret, rawBody)).toUpperCase();

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).resolves.toEqual({
      action: 'opened',
    });
  });

  it('throws when the raw body contains invalid JSON after successful verification', async () => {
    const secret = 'github_test_secret';
    const rawBody = '{not-json';
    const header = await createGitHubSignatureHeader(secret, rawBody);

    await expect(verify(createWebhookEvent(rawBody, header), { secret })).rejects.toThrow(
      'GitHub webhook body contained invalid JSON.',
    );
  });
});
