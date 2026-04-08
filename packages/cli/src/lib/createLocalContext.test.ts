import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createLocalContext } from './createLocalContext';

const originalConsoleLog = console.log;
const originalConsoleWarn = console.warn;
const originalConsoleError = console.error;

beforeEach(() => {
  console.log = vi.fn();
  console.warn = vi.fn();
  console.error = vi.fn();
});

afterEach(() => {
  console.log = originalConsoleLog;
  console.warn = originalConsoleWarn;
  console.error = originalConsoleError;
});

describe('createLocalContext', () => {
  it('returns a context with env and log', () => {
    const ctx = createLocalContext('payment');

    expect(ctx.env).toEqual({});
    expect(ctx.log).toBeDefined();
    expect(typeof ctx.log.info).toBe('function');
    expect(typeof ctx.log.warn).toBe('function');
    expect(typeof ctx.log.error).toBe('function');
  });

  it('logs info messages with prefix and message', () => {
    const ctx = createLocalContext('payment');

    ctx.log.info('processing payment');

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*INFO.*processing payment/),
      '',
    );
  });

  it('logs info messages with metadata', () => {
    const ctx = createLocalContext('payment');

    const meta = { userId: '123' };

    ctx.log.info('processing payment', meta);

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*INFO.*processing payment/),
      meta,
    );
  });

  it('logs warn messages with prefix and message', () => {
    const ctx = createLocalContext('payment');

    ctx.log.warn('something looks off');

    expect(console.warn).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*WARN.*something looks off/),
      '',
    );
  });

  it('logs error messages with prefix and message', () => {
    const ctx = createLocalContext('payment');

    ctx.log.error('something failed');

    expect(console.error).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*ERROR.*something failed/),
      '',
    );
  });

  it('passes metadata correctly for warn and error', () => {
    const ctx = createLocalContext('payment');

    const meta = { reason: 'invalid input' };

    ctx.log.warn('warning', meta);
    ctx.log.error('error', meta);

    expect(console.warn).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*WARN.*warning/),
      meta,
    );

    expect(console.error).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*ERROR.*error/),
      meta,
    );
  });

  it('handles undefined metadata by passing empty string', () => {
    const ctx = createLocalContext('payment');

    ctx.log.info('no meta');

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[payment\].*INFO.*no meta/),
      '',
    );
  });
});