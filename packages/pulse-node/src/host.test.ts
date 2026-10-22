import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

import { beforeAll, describe, expect, it } from 'vitest';

import { createExecutionServer } from '../../../examples/pulse-node/server.ts';
import { Pulse, pulseBin, repoRoot, sampleProgram } from './host.ts';

const twoEffects = `import { effect } from '@trigora/sdk';

export default async function program() {
  await effect('first', () => 1);
  return await effect('second', () => 2);
}
`;

beforeAll(() => {
  const bin = pulseBin();
  if (!fs.existsSync(bin)) {
    execFileSync('cargo', ['build', '-p', 'trigora-pulse'], {
      cwd: repoRoot(),
      stdio: 'inherit',
      env: { ...process.env, CARGO_TARGET_DIR: path.join(repoRoot(), 'target') },
    });
  }
}, 180_000);

describe('pulse node adapter', () => {
  it('runs a compiled sdk effect and returns an accepted execution id', async () => {
    const calls: string[] = [];
    const pulse = await Pulse.open({
      source: sampleProgram,
      bin: pulseBin(),
      onEffect: (key) => calls.push(key),
    });
    try {
      const executionId = await pulse.start('exec-1');
      expect(executionId).toBe('exec-1');
      await waitFor(() => calls.includes('generate'));
      expect(calls).toEqual(['generate']);
      await expect(pulse.start('exec-1')).rejects.toThrow(/already in use/);
    } finally {
      await pulse.close();
    }
  });

  it('does not give a v2 registration the v1 effect', async () => {
    const calls: string[] = [];
    const pulse = await Pulse.open({
      source: sampleProgram,
      bin: pulseBin(),
      onEffect: (key) => calls.push(key),
    });
    try {
      const peer = await pulse.attachPeer('other-hash', ['generate']);
      try {
        await pulse.start();
        await waitFor(() => calls.includes('generate'));
        await new Promise((resolve) => setTimeout(resolve, 50));
        expect(peer.effects()).toEqual([]);
      } finally {
        peer.close();
      }
    } finally {
      await pulse.close();
    }
  });

  it('retries the outstanding effect after the adapter reconnects', async () => {
    const calls: string[] = [];
    let pulse!: Pulse;
    let dropped = false;
    pulse = await Pulse.open({
      source: twoEffects,
      bin: pulseBin(),
      onEffect: (key) => {
        calls.push(key);
        if (key === 'second' && !dropped) {
          dropped = true;
          void pulse.disconnectAdapter();
        }
      },
    });
    try {
      await pulse.start();
      await waitFor(() => dropped);
      await pulse.reconnect();
      await waitFor(() => calls.filter((key) => key === 'second').length >= 2);
      expect(calls.filter((key) => key === 'first')).toEqual(['first']);
      expect(calls.filter((key) => key === 'second').length).toBeGreaterThanOrEqual(2);
    } finally {
      await pulse.close();
    }
  });

  it('returns the execution id from the http route', async () => {
    const pulse = await Pulse.open({ source: sampleProgram, bin: pulseBin() });
    const server = createExecutionServer(pulse);
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
    try {
      const address = server.address();
      if (!address || typeof address === 'string') {
        throw new Error('missing port');
      }
      const response = await fetch(`http://127.0.0.1:${address.port}/executions`, {
        method: 'POST',
      });
      expect(response.status).toBe(200);
      const body = (await response.json()) as { executionId: string };
      expect(body.executionId.length).toBeGreaterThan(0);
    } finally {
      server.close();
      await pulse.close();
    }
  });
});

async function waitFor(ready: () => boolean): Promise<void> {
  const started = Date.now();
  while (!ready()) {
    if (Date.now() - started > 5_000) {
      throw new Error('timed out waiting for the adapter');
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}
