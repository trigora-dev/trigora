import { spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { CliDisplayError } from '../cliOutput';
import { parseTrigoraToml } from './loadConfig';
import {
  extractRustEffects,
  runRustEffect,
  rustEffectBinary,
  rustEffectWasm,
  rustHarnessSources,
} from './rustEffects';

const cargo = spawnSync('cargo', ['--version'], { encoding: 'utf8' });
const hasCargo = cargo.status === 0;

const APPROVAL = `use trigora::{effect, wait_for_event};

pub struct Approval {
    pub result: f64,
    pub review: bool,
}

pub async fn main() -> Result<Approval, String> {
    let amount: f64 = 42.0;
    let result = effect("generate", move || amount).await?;
    let review: bool = wait_for_event("approved").await?;
    Ok(Approval { result: result, review: review })
}
`;

describe('extractRustEffects', () => {
  it('reads a move capture and ignores a local function named effect', () => {
    const effects = extractRustEffects(APPROVAL);
    expect(effects).toEqual([
      {
        key: 'generate',
        body: 'amount',
        captures: [{ name: 'amount', ty: 'f64' }],
      },
    ]);

    expect(
      extractRustEffects(`fn effect(x: f64) -> f64 { x }
async fn run() -> f64 { effect(3.0) }`),
    ).toEqual([]);
  });

  it('follows an effect alias', () => {
    const effects = extractRustEffects(`use trigora::effect as fx;
pub async fn run() -> Result<bool, String> {
    let ready: bool = true;
    let value = fx("check", move || ready).await?;
    Ok(value)
}
`);
    expect(effects[0]?.key).toBe('check');
    expect(effects[0]?.captures).toEqual([{ name: 'ready', ty: 'bool' }]);
  });
});

describe('parseTrigoraToml', () => {
  it('reads project name and include globs', () => {
    const parsed = parseTrigoraToml(`[project]
name = "approval"
programs = ["src/**/*.rs"]
`);
    expect(parsed.projectName).toBe('approval');
    expect(parsed.programs).toEqual(['src/**/*.rs']);
    expect(parsed.triggers).toEqual([]);
  });

  it('rejects a toml file with no programs', () => {
    expect(() => parseTrigoraToml('[project]\nname = "approval"\n')).toThrow(/programs/);
  });
});

describe('rust effect harness', () => {
  const tempDirs: string[] = [];

  afterEach(async () => {
    await Promise.all(
      tempDirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })),
    );
  });

  it('binds a captured f64 in the harness source', () => {
    const effects = extractRustEffects(APPROVAL);
    const sources = rustHarnessSources(effects, '');
    expect(sources.handlersRs).toContain('require_f64(input, "amount")');
    expect(sources.cargoToml).toContain('serde_json = "1"');
  });

  it('builds the harness and returns the captured value', async () => {
    if (!hasCargo) {
      return;
    }
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-rust-effect-'));
    tempDirs.push(root);
    const binary = rustEffectBinary({
      rootDir: root,
      programId: 'approval',
      file: 'src/lib.rs',
      source: APPROVAL,
    });
    expect(runRustEffect(binary, 'generate', { amount: 42 })).toBe(42);
  }, 120_000);

  it('runs the wasm pointer/length ABI for a captured f64', async () => {
    if (!hasCargo) {
      return;
    }
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-rust-wasm-'));
    tempDirs.push(root);
    const wasm = rustEffectWasm({
      rootDir: root,
      programId: 'approval',
      file: 'src/lib.rs',
      source: APPROVAL,
    });
    const webAssembly = (
      globalThis as unknown as {
        WebAssembly: {
          instantiate(bytes: Buffer): Promise<{ instance: { exports: object } }>;
        };
      }
    ).WebAssembly;
    const { instance } = await webAssembly.instantiate(wasm);
    const exports = instance.exports as {
      alloc: (len: number) => number;
      handle: (ptr: number, len: number) => number;
      out_len: () => number;
      memory: { buffer: ArrayBuffer };
    };
    const payload = new TextEncoder().encode(
      JSON.stringify({ key: 'generate', input: { amount: 42 } }),
    );
    const ptr = exports.alloc(payload.byteLength);
    new Uint8Array(exports.memory.buffer, ptr, payload.byteLength).set(payload);
    const outPtr = exports.handle(ptr, payload.byteLength);
    const text = new TextDecoder().decode(
      new Uint8Array(exports.memory.buffer, outPtr, exports.out_len()),
    );
    expect(JSON.parse(text)).toEqual({ result: 42 });
  }, 120_000);

  it('compiles the approval program when the rust frontend is installed', async () => {
    if (!hasCargo) {
      return;
    }
    const { compileRustProgram } = await import('./compiler');
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-rust-compile-'));
    tempDirs.push(root);
    try {
      const compiled = await compileRustProgram(APPROVAL, 'src/lib.rs', {
        rootDir: root,
        programId: 'approval',
      });
      expect(compiled.language).toBe('rust');
      expect(compiled.artifactHash).toMatch(/^[a-f0-9]{64}$/);
      expect(compiled.effects.generate?.({ amount: 42 })).toBe(42);
    } catch (error) {
      if (error instanceof CliDisplayError && error.title === 'Rust compiler unavailable') {
        return;
      }
      throw error;
    }
  }, 120_000);

  it('rejects a file whose only async function is run', async () => {
    const { compileRustProgram } = await import('./compiler');
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-rust-run-'));
    tempDirs.push(root);
    try {
      await compileRustProgram(
        `pub async fn run() -> Result<String, String> {\n    Ok(String::from("hello"))\n}\n`,
        'src/lib.rs',
        { rootDir: root, programId: 'approval' },
      );
      throw new Error('expected compilation to fail');
    } catch (error) {
      if (error instanceof CliDisplayError && error.title === 'Rust compiler unavailable') {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      expect(message).toMatch(/exactly one `pub async fn main`/);
    }
  }, 120_000);
});
