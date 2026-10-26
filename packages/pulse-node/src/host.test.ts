import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
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
  it('exits when durability is not configured', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pulse-closed-'));
    const artifact = path.join(dir, 'artifact.json');
    fs.writeFileSync(artifact, '{}');
    const env = { ...process.env };
    delete env.TRIGORA_TOKEN;
    delete env.TRIGORA_API_BASE_URL;
    delete env.TRIGORA_PULSE_AUTHORITY_URL;
    delete env.TRIGORA_PULSE_WORKSPACE_ID;
    delete env.TRIGORA_PULSE_WORKSPACE_TOKEN;
    const result = spawnSync(pulseBin(), [dir, artifact], { encoding: 'utf8', env });
    expect(result.status).toBe(2);
    expect(result.stderr.toLowerCase()).toContain('durability is not configured');
    expect(fs.existsSync(path.join(dir, 'pulse.sock'))).toBe(false);
  });

  it('runs a compiled sdk effect and returns an accepted execution id', async () => {
    const calls: string[] = [];
    const pulse = await Pulse.open({
      source: sampleProgram,
      bin: pulseBin(),
      ephemeral: true,
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
      ephemeral: true,
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
      ephemeral: true,
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
    const pulse = await Pulse.open({ source: sampleProgram, bin: pulseBin(), ephemeral: true });
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

const oneEffect = `import { effect } from '@trigora/sdk';

export default async function program() {
  return await effect('work', () => 1);
}
`;

describe('async effect callbacks', () => {
  it('runs another execution while a promise is pending', async () => {
    let release: (value: number) => void = () => undefined;
    const seen: string[] = [];
    const pulse = await Pulse.open({
      source: oneEffect,
      bin: pulseBin(),
      ephemeral: true,
      onEffect: (_key, executionId) => seen.push(executionId),
    });
    let first = true;
    pulse.setHandler('work', () => {
      if (first) {
        first = false;
        return new Promise<number>((resolve) => {
          release = resolve;
        });
      }
      return 1;
    });
    try {
      await pulse.start('slow');
      await waitFor(() => seen.includes('slow'));
      await pulse.start('fast');
      await waitFor(() => seen.includes('fast'));
      expect(seen).toEqual(['slow', 'fast']);
      release(1);
    } finally {
      await pulse.close();
    }
  });

  it('lets a synchronous callback block the other execution', async () => {
    const seen: number[] = [];
    const pulse = await Pulse.open({
      source: oneEffect,
      bin: pulseBin(),
      ephemeral: true,
      onEffect: () => seen.push(Date.now()),
    });
    pulse.setHandler('work', () => {
      const started = Date.now();
      while (Date.now() - started < 120) {
        // A synchronous callback occupies this Node process.
      }
      return 1;
    });
    try {
      await pulse.start('first');
      await pulse.start('second');
      await waitFor(() => seen.length >= 2);
      const first = seen[0] ?? 0;
      const second = seen[1] ?? 0;
      expect(second - first).toBeGreaterThanOrEqual(100);
    } finally {
      await pulse.close();
    }
  });

  it('does not journal a promise that resolves after its attempt was replaced', async () => {
    let release: (value: number) => void = () => undefined;
    const attempts: string[] = [];
    const pulse = await Pulse.open({
      source: oneEffect,
      bin: pulseBin(),
      ephemeral: true,
      timings: true,
      effectTimeoutMs: 300,
      onEffect: (_key, _executionId, attemptId) => attempts.push(attemptId),
    });
    let first = true;
    pulse.setHandler('work', () => {
      if (first) {
        first = false;
        return new Promise<number>((resolve) => {
          release = resolve;
        });
      }
      return 2;
    });
    try {
      await pulse.start('late');
      await waitFor(() => attempts.length >= 2);
      release(1);
      await waitFor(() => pulse.hostSpans().some((span) => span.attemptId.endsWith(':2')));
      expect(pulse.hostSpans().some((span) => span.attemptId.endsWith(':1'))).toBe(false);
    } finally {
      await pulse.close();
    }
  }, 15_000);
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
