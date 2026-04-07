import { describe, expect, it } from 'vitest';
import type { Trigger } from './trigger';

describe('Trigger types', () => {
  it('accepts a manual trigger', () => {
    const trigger: Trigger = { type: 'manual' };

    expect(trigger.type).toBe('manual');
  });

  it('accepts a webhook trigger', () => {
    const trigger: Trigger = {
      type: 'webhook',
      event: 'stripe.payment_succeeded',
    };

    expect(trigger.type).toBe('webhook');

    if (trigger.type === 'webhook') {
      expect(trigger.event).toBe('stripe.payment_succeeded');
    }
  });

  it('accepts a cron trigger', () => {
    const trigger: Trigger = {
      type: 'cron',
      cron: '0 9 * * *',
    };

    expect(trigger.type).toBe('cron');

    if (trigger.type === 'cron') {
      expect(trigger.cron).toBe('0 9 * * *');
    }
  });
});
