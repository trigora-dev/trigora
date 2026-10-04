import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const here = path.dirname(fileURLToPath(import.meta.url));
const cliRoot = path.resolve(here, '..');
const repoRoot = path.resolve(cliRoot, '../..');
const platform = `${process.platform}-${process.arch}`;
const extension = process.platform === 'win32' ? '.exe' : '';

function firstExisting(candidates: string[]): string | undefined {
  return candidates.find((candidate) => fs.existsSync(candidate));
}

function copyExecutable(source: string, destination: string) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(source, destination);
  fs.chmodSync(destination, 0o755);
}

describe('installed package layout', () => {
  it('starts through the shim and reaches ready', async () => {
    const built = spawnSync(process.execPath, ['scripts/build-adapter.mjs'], {
      cwd: cliRoot,
      encoding: 'utf8',
    });
    expect(built.status).toBe(0);
    const trigora = firstExisting([
      path.join(cliRoot, 'vendor', 'trigora', platform, `trigora${extension}`),
      path.join(repoRoot, 'target', 'release', `trigora${extension}`),
      path.join(repoRoot, 'target', 'debug', `trigora${extension}`),
    ]);
    const local = firstExisting([
      path.join(cliRoot, 'vendor', 'trigora-local', platform, `trigora-local${extension}`),
      path.join(repoRoot, 'target', 'release', `trigora-local${extension}`),
      path.join(repoRoot, 'target', 'debug', `trigora-local${extension}`),
    ]);
    expect(trigora, 'build trigora before the layout smoke test').toBeTruthy();
    expect(local, 'build trigora-local before the layout smoke test').toBeTruthy();

    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'trigora-layout-'));
    const packageRoot = path.join(root, 'node_modules', 'trigora');
    copyExecutable(
      trigora!,
      path.join(packageRoot, 'vendor', 'trigora', platform, `trigora${extension}`),
    );
    copyExecutable(
      local!,
      path.join(packageRoot, 'vendor', 'trigora-local', platform, `trigora-local${extension}`),
    );
    const rustCompiler = path.join(
      packageRoot,
      'vendor',
      'tcc-rust-compile',
      platform,
      `tcc-rust-compile${extension}`,
    );
    const existingCompiler = firstExisting([
      path.join(cliRoot, 'vendor', 'tcc-rust-compile', platform, `tcc-rust-compile${extension}`),
      path.join(repoRoot, 'tcc-engine', 'target', 'release', `tcc-rust-compile${extension}`),
    ]);
    if (existingCompiler) {
      copyExecutable(existingCompiler, rustCompiler);
    } else {
      fs.mkdirSync(path.dirname(rustCompiler), { recursive: true });
      fs.writeFileSync(rustCompiler, "#!/bin/sh\nprintf '%s\\n' '{}'\n");
      fs.chmodSync(rustCompiler, 0o755);
    }
    fs.mkdirSync(path.join(packageRoot, 'bin'), { recursive: true });
    fs.copyFileSync(path.join(cliRoot, 'bin', 'shim.js'), path.join(packageRoot, 'bin', 'shim.js'));
    fs.mkdirSync(path.join(packageRoot, 'helper'), { recursive: true });
    fs.copyFileSync(
      path.join(cliRoot, 'helper', 'node-helper.js'),
      path.join(packageRoot, 'helper', 'node-helper.js'),
    );
    fs.copyFileSync(
      path.join(cliRoot, 'helper', 'ts-adapter.js'),
      path.join(packageRoot, 'helper', 'ts-adapter.js'),
    );
    const shippedHelper = fs.readFileSync(
      path.join(packageRoot, 'helper', 'node-helper.js'),
      'utf8',
    );
    const shippedAdapter = fs.readFileSync(
      path.join(packageRoot, 'helper', 'ts-adapter.js'),
      'utf8',
    );
    expect(shippedHelper).toContain('./ts-adapter.js');
    expect(shippedHelper).not.toMatch(/from ['"][^'"]+\.ts['"]/);
    expect(shippedAdapter).not.toMatch(/from ['"][^'"]+\.ts['"]/);
    fs.writeFileSync(path.join(packageRoot, 'package.json'), JSON.stringify({ type: 'module' }));
    fs.symlinkSync(path.join(cliRoot, 'node_modules'), path.join(packageRoot, 'node_modules'));

    const project = path.join(root, 'project');
    fs.mkdirSync(path.join(project, 'src'), { recursive: true });
    fs.writeFileSync(
      path.join(project, 'trigora.toml'),
      '[project]\nname = "demo"\nprograms = ["src/**/*.ts"]\n',
    );
    fs.writeFileSync(
      path.join(project, 'src', 'program.ts'),
      'export default async function program() {\n  return 1;\n}\n',
    );

    const env = { ...process.env };
    delete env.TRIGORA_BIN;
    delete env.TRIGORA_LOCAL_BIN;
    delete env.TRIGORA_NODE_HELPER;
    delete env.TRIGORA_RUST_COMPILER_BIN;
    const child = spawn(process.execPath, [path.join(packageRoot, 'bin', 'shim.js'), 'dev'], {
      cwd: project,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    child.stdout?.on('data', (chunk: Buffer) => {
      output += chunk.toString();
    });
    child.stderr?.on('data', (chunk: Buffer) => {
      output += chunk.toString();
    });
    const ready = await waitFor(
      () => output.includes('Local runtime ready'),
      child,
      () => output,
    );
    expect(ready).toContain('Local runtime ready');
    const port = Number(ready.match(/http:\/\/127\.0\.0\.1:(\d+)/)?.[1]);
    expect(port).toBeGreaterThan(0);

    fs.writeFileSync(
      path.join(project, 'src', 'program.ts'),
      'export default async function program() {\n  return 2;\n}\n',
    );
    const reloaded = await waitFor(
      () => output.includes('Reloaded'),
      child,
      () => output,
    );
    expect(reloaded).toContain('Reloaded');

    child.kill('SIGTERM');
    const status = await new Promise<number | null>((resolve) => {
      child.once('exit', (code) => resolve(code));
    });
    expect(status).toBe(0);
    await expect(connect(port)).rejects.toThrow();
    const processes = spawnSyncText('ps', ['-ax', '-o', 'args=']);
    expect(processes).not.toContain(path.join(packageRoot, 'vendor', 'trigora-local'));
    expect(processes).not.toContain(path.join(packageRoot, 'helper', 'node-helper.js'));
    fs.rmSync(root, { recursive: true, force: true });
  }, 60_000);
});

function spawnSyncText(command: string, args: string[]): string {
  return spawnSync(command, args, { encoding: 'utf8' }).stdout ?? '';
}

function connect(port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    socket.once('connect', () => {
      socket.end();
      resolve();
    });
    socket.once('error', reject);
  });
}

async function waitFor(
  ready: () => boolean,
  child: ChildProcess,
  output: () => string,
): Promise<string> {
  const started = Date.now();
  while (!ready()) {
    if (child.exitCode !== null) {
      throw new Error(output());
    }
    if (Date.now() - started > 40_000) {
      child.kill('SIGTERM');
      throw new Error(output());
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return output();
}
