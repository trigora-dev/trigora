import { describe, expect, it, vi } from 'vitest';
import { validateFlowModule } from './validateFlowModule';

describe('validateFlowModule', () => {
  it('returns a valid manual flow', () => {
    const run = vi.fn();

    const flow = validateFlowModule('flows/hello.ts', {
      id: 'hello',
      trigger: { type: 'manual' },
      run,
    });

    expect(flow.id).toBe('hello');
    expect(flow.trigger).toEqual({ type: 'manual' });
    expect(flow.run).toBe(run);
  });

  it('returns a valid webhook flow', () => {
    const run = vi.fn();

    const flow = validateFlowModule('flows/payment.ts', {
      id: 'payment',
      trigger: { type: 'webhook', event: 'stripe.payment_succeeded' },
      run,
    });

    expect(flow.id).toBe('payment');
    expect(flow.trigger).toEqual({
      type: 'webhook',
      event: 'stripe.payment_succeeded',
    });
    expect(flow.run).toBe(run);
  });

  it('returns a valid cron flow', () => {
    const run = vi.fn();

    const flow = validateFlowModule('flows/daily.ts', {
      id: 'daily',
      trigger: { type: 'cron', cron: '0 9 * * *' },
      run,
    });

    expect(flow.id).toBe('daily');
    expect(flow.trigger).toEqual({
      type: 'cron',
      cron: '0 9 * * *',
    });
    expect(flow.run).toBe(run);
  });

  it('throws when default export is undefined', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', undefined);
    }).toThrow(
      'No default export found in "flows/hello.ts". Expected a default exported flow.',
    );
  });

  it('throws when default export is not an object', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', 'not-an-object');
    }).toThrow(
      'Invalid flow in "flows/hello.ts": default export must be an object.',
    );
  });

  it('throws when id is missing', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        trigger: { type: 'manual' },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": "id" must be a non-empty string.',
    );
  });

  it('throws when id is empty', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        id: '',
        trigger: { type: 'manual' },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": "id" must be a non-empty string.',
    );
  });

  it('throws when run is missing', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        id: 'hello',
        trigger: { type: 'manual' },
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": "run" must be a function.',
    );
  });

  it('throws when trigger is missing', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        id: 'hello',
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": "trigger" must be an object.',
    );
  });

  it('throws when trigger.type is missing', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        id: 'hello',
        trigger: {},
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": "trigger.type" must be a string.',
    );
  });

  it('throws when trigger type is unsupported', () => {
    expect(() => {
      validateFlowModule('flows/hello.ts', {
        id: 'hello',
        trigger: { type: 'unknown' },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/hello.ts": unsupported trigger type "unknown". Expected "manual", "webhook", or "cron".',
    );
  });

  it('allows webhook event to be omitted', () => {
    const run = vi.fn();
  
    const flow = validateFlowModule('flows/payment.ts', {
      id: 'payment',
      trigger: { type: 'webhook' },
      run,
    });
  
    expect(flow.id).toBe('payment');
    expect(flow.trigger).toEqual({ type: 'webhook' });
    expect(flow.run).toBe(run);
  });
  
  it('throws when webhook event is not a string', () => {
    expect(() => {
      validateFlowModule('flows/payment.ts', {
        id: 'payment',
        trigger: { type: 'webhook', event: 123 },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/payment.ts": "trigger.event" must be a string when provided.',
    );
  });
  
  it('allows webhook event to be an empty string', () => {
    const run = vi.fn();
  
    const flow = validateFlowModule('flows/payment.ts', {
      id: 'payment',
      trigger: { type: 'webhook', event: '   ' },
      run,
    });
  
    expect(flow.id).toBe('payment');
    expect(flow.trigger).toEqual({
      type: 'webhook',
      event: '   ',
    });
    expect(flow.run).toBe(run);
  });

  it('throws when cron string is missing', () => {
    expect(() => {
      validateFlowModule('flows/daily.ts', {
        id: 'daily',
        trigger: { type: 'cron' },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/daily.ts": cron triggers require a non-empty "trigger.cron" string.',
    );
  });

  it('throws when cron string is empty', () => {
    expect(() => {
      validateFlowModule('flows/daily.ts', {
        id: 'daily',
        trigger: { type: 'cron', cron: '   ' },
        run: vi.fn(),
      });
    }).toThrow(
      'Invalid flow in "flows/daily.ts": cron triggers require a non-empty "trigger.cron" string.',
    );
  });
});