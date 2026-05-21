import type { DeleteFlowSecretResponse, FlowRecord, ListSecretsResponse } from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createDeployApiClient, type DeployApiClient } from '../lib/createDeployApiClient';
import { confirmAction, promptForSecretValue } from '../lib/interactive';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './secrets';

vi.mock('../lib/createDeployApiClient', async () => {
  const actual = await vi.importActual<typeof import('../lib/createDeployApiClient')>(
    '../lib/createDeployApiClient',
  );

  return {
    ...actual,
    createDeployApiClient: vi.fn(),
  };
});

vi.mock('../lib/interactive', () => ({
  confirmAction: vi.fn(),
  promptForSecretValue: vi.fn(),
}));

const originalConsoleLog = console.log;
const originalConsoleWarn = console.warn;
const originalEnv = { ...process.env };
const mockedCreateDeployApiClient = vi.mocked(createDeployApiClient);
const mockedConfirmAction = vi.mocked(confirmAction);
const mockedPromptForSecretValue = vi.mocked(promptForSecretValue);

const stripeFlow = {
  id: 'stripe-checkout',
  slug: 'stripe-checkout',
  status: 'ready',
  trigger: 'webhook' as const,
  routePath: '/stripe-checkout',
  endpoint: 'https://acme.trigora.dev/stripe-checkout',
  createdAt: '2026-05-03T10:00:00.000Z',
} satisfies FlowRecord;

const webhookSecret = {
  flowSlug: stripeFlow.slug,
  name: 'STRIPE_WEBHOOK_SECRET',
  createdAt: '2026-05-03T12:00:00.000Z',
  updatedAt: '2026-05-03T12:00:00.000Z',
} satisfies ListSecretsResponse['secrets'][number];

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlow: vi.fn(),
    deleteFlowSecret: vi.fn().mockResolvedValue({
      ok: true,
      deleted: true,
      name: 'STRIPE_WEBHOOK_SECRET',
    } satisfies DeleteFlowSecretResponse),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    getFlow: vi.fn().mockResolvedValue(stripeFlow),
    getInvocation: vi.fn(),
    listInvocations: vi.fn(),
    listFlowSecrets: vi.fn().mockResolvedValue([webhookSecret]),
    listFlows: vi.fn(),
    setFlowSecret: vi.fn().mockResolvedValue(webhookSecret),
    whoAmI: vi.fn(),
    ...overrides,
  };
}

beforeEach(() => {
  console.log = vi.fn();
  console.warn = vi.fn();
  process.env = {
    ...originalEnv,
    TRIGORA_DEPLOY_TOKEN: 'secret-token',
  };
  mockedCreateDeployApiClient.mockReset();
  mockedCreateDeployApiClient.mockReturnValue(createMockApiClient());
  mockedConfirmAction.mockReset();
  mockedPromptForSecretValue.mockReset();
});

afterEach(() => {
  console.log = originalConsoleLog;
  console.warn = originalConsoleWarn;
  process.env = originalEnv;
});

describe('secrets commands', () => {
  it('validates secret names before making API calls', async () => {
    await expect(
      setSecretCommand({
        flow: stripeFlow.slug,
        name: '1INVALID',
        value: 'super-secret',
      }),
    ).rejects.toMatchObject({
      title: 'Request failed',
      details: expect.arrayContaining([
        expect.objectContaining({
          label: 'Reason',
          value: 'Invalid secret name "1INVALID".',
        }),
      ]),
    });
  });

  it('sets a hosted flow secret for the resolved flow', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      setSecretCommand({
        flow: stripeFlow.slug,
        name: 'STRIPE_WEBHOOK_SECRET',
        value: 'super-secret',
      }),
    ).resolves.toEqual(webhookSecret);

    expect(apiClient.setFlowSecret).toHaveBeenCalledWith(stripeFlow.slug, {
      name: 'STRIPE_WEBHOOK_SECRET',
      value: 'super-secret',
    });
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(
        /Setting secret STRIPE_WEBHOOK_SECRET for flow .*stripe-checkout.*\.\.\./,
      ),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Secret set/));
    expect(console.log).toHaveBeenCalledWith('Available in code as:');
    expect(console.log).toHaveBeenCalledWith('ctx.env.STRIPE_WEBHOOK_SECRET');
    expect(console.log).not.toHaveBeenCalledWith(expect.stringContaining('super-secret'));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Resolving flow/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Setting secret\.\.\./));
  });

  it('prompts for the secret value when one is not provided', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);
    mockedPromptForSecretValue.mockResolvedValue('prompted-secret');

    await setSecretCommand({
      flow: stripeFlow.slug,
      name: 'STRIPE_WEBHOOK_SECRET',
    });

    expect(mockedPromptForSecretValue).toHaveBeenCalledWith('STRIPE_WEBHOOK_SECRET');
    expect(apiClient.setFlowSecret).toHaveBeenCalledWith(stripeFlow.slug, {
      name: 'STRIPE_WEBHOOK_SECRET',
      value: 'prompted-secret',
    });
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(
        /Setting secret STRIPE_WEBHOOK_SECRET for flow .*stripe-checkout.*\.\.\./,
      ),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Waiting for secret value/));
  });

  it('lists secret metadata without printing values', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-05-03T14:00:00.000Z'));

    const apiClient = createMockApiClient({
      listFlowSecrets: vi.fn().mockResolvedValue([
        webhookSecret,
        {
          flowSlug: stripeFlow.slug,
          name: 'RESEND_API_KEY',
          createdAt: '2026-05-02T12:00:00.000Z',
          updatedAt: '2026-05-02T14:00:00.000Z',
        } satisfies ListSecretsResponse['secrets'][number],
      ]),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listSecretsCommand({
        flow: stripeFlow.slug,
      }),
    ).resolves.toEqual([
      webhookSecret,
      {
        flowSlug: stripeFlow.slug,
        name: 'RESEND_API_KEY',
        createdAt: '2026-05-02T12:00:00.000Z',
        updatedAt: '2026-05-02T14:00:00.000Z',
      },
    ]);

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Secrets for flow .*stripe-checkout.*:/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/STRIPE_WEBHOOK_SECRET\s+set 2h ago/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/RESEND_API_KEY\s+set 1d ago/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringContaining('super-secret'));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Fetching flow secrets/));

    vi.useRealTimers();
  });

  it('shows an empty state when a flow has no secrets', async () => {
    const apiClient = createMockApiClient({
      listFlowSecrets: vi.fn().mockResolvedValue([]),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listSecretsCommand({
        flow: stripeFlow.slug,
      }),
    ).resolves.toEqual([]);

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/No secrets set for flow .*stripe-checkout.*\./),
    );
  });

  it('skips deletion when confirmation is declined', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);
    mockedConfirmAction.mockResolvedValue(false);

    await expect(
      deleteSecretCommand({
        flow: stripeFlow.slug,
        name: 'STRIPE_WEBHOOK_SECRET',
      }),
    ).resolves.toBeNull();

    expect(mockedConfirmAction).toHaveBeenCalledWith(
      'Delete secret "STRIPE_WEBHOOK_SECRET" for flow "stripe-checkout"?',
    );
    expect(apiClient.deleteFlowSecret).not.toHaveBeenCalled();
    expect(console.warn).toHaveBeenCalledWith(expect.stringMatching(/Skipped deleting secret/));
  });

  it('deletes without prompting when --yes is used', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      deleteSecretCommand({
        flow: stripeFlow.slug,
        name: 'STRIPE_WEBHOOK_SECRET',
        yes: true,
      }),
    ).resolves.toEqual({
      ok: true,
      deleted: true,
      name: 'STRIPE_WEBHOOK_SECRET',
    });

    expect(mockedConfirmAction).not.toHaveBeenCalled();
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(
        /Deleting secret .*STRIPE_WEBHOOK_SECRET.* for flow .*stripe-checkout.*\.\.\./,
      ),
    );
    expect(apiClient.deleteFlowSecret).toHaveBeenCalledWith(
      stripeFlow.slug,
      'STRIPE_WEBHOOK_SECRET',
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/✔ Secret deleted: STRIPE_WEBHOOK_SECRET/),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Deleting secret\.\.\./));
  });

  it('throws a polished error when the deploy token is missing', async () => {
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    await expect(
      listSecretsCommand({
        flow: stripeFlow.slug,
      }),
    ).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });
});
