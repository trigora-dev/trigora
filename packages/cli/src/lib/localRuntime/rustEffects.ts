import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

import { CliDisplayError } from '../cliOutput';

const DURABLE_CRATES = new Set(['trigora', 'tcc_rust_prelude']);
const RUST_KEYWORDS = new Set([
  'as',
  'async',
  'await',
  'break',
  'const',
  'continue',
  'crate',
  'else',
  'enum',
  'extern',
  'false',
  'fn',
  'for',
  'if',
  'impl',
  'in',
  'let',
  'loop',
  'match',
  'mod',
  'move',
  'mut',
  'pub',
  'ref',
  'return',
  'self',
  'Self',
  'static',
  'struct',
  'super',
  'trait',
  'true',
  'type',
  'unsafe',
  'use',
  'where',
  'while',
  'abstract',
  'become',
  'box',
  'do',
  'final',
  'macro',
  'override',
  'priv',
  'typeof',
  'unsized',
  'virtual',
  'yield',
  'try',
]);

export type RustCapture = {
  name: string;
  ty: 'f64' | 'bool' | 'String';
};

export type RustEffectClosure = {
  key: string;
  body: string;
  captures: RustCapture[];
};

export type RustHarnessFiles = {
  cargoToml: string;
  mainRs: string;
  libRs: string;
  handlersRs: string;
};

function maskCommentsAndStrings(source: string): string {
  let output = '';
  let index = 0;
  while (index < source.length) {
    const current = source[index];
    const next = source[index + 1];
    if (current === '/' && next === '/') {
      output += '  ';
      index += 2;
      while (index < source.length && source[index] !== '\n') {
        output += ' ';
        index += 1;
      }
      continue;
    }
    if (current === '/' && next === '*') {
      output += '  ';
      index += 2;
      while (index < source.length && !(source[index] === '*' && source[index + 1] === '/')) {
        output += source[index] === '\n' ? '\n' : ' ';
        index += 1;
      }
      output += '  ';
      index += 2;
      continue;
    }
    if (current === '"') {
      output += ' ';
      index += 1;
      while (index < source.length && source[index] !== '"') {
        if (source[index] === '\\') {
          output += '  ';
          index += 2;
          continue;
        }
        output += source[index] === '\n' ? '\n' : ' ';
        index += 1;
      }
      output += ' ';
      index += 1;
      continue;
    }
    output += current;
    index += 1;
  }
  return output;
}

function effectAliases(masked: string): Set<string> {
  const aliases = new Set<string>();
  const pattern = /use\s+(trigora|tcc_rust_prelude)::([^;]+);/g;
  for (const match of masked.matchAll(pattern)) {
    if (!match[1] || !DURABLE_CRATES.has(match[1]) || !match[2]) {
      continue;
    }
    const tree = match[2].trim();
    const parts = tree.startsWith('{') ? tree.slice(1, tree.lastIndexOf('}')).split(',') : [tree];
    for (const part of parts) {
      const [name, alias] = part.trim().split(/\s+as\s+/);
      if (name === 'effect') {
        aliases.add((alias ?? name).trim());
      }
    }
  }
  return aliases;
}

function isIdentChar(char: string | undefined): boolean {
  return Boolean(char && /[A-Za-z0-9_]/.test(char));
}

function readIdent(source: string, index: number): string | undefined {
  if (!/[A-Za-z_]/.test(source[index] ?? '')) {
    return undefined;
  }
  let end = index + 1;
  while (isIdentChar(source[end])) {
    end += 1;
  }
  return source.slice(index, end);
}

function skipSpace(source: string, index: number): number {
  while (index < source.length && /\s/.test(source[index] ?? '')) {
    index += 1;
  }
  return index;
}

function splitArguments(source: string, openParen: number): { args: string[]; end: number } {
  const args: string[] = [];
  let start = openParen + 1;
  let depth = 1;
  for (let index = openParen + 1; index < source.length; index += 1) {
    const char = source[index];
    if (char === '(' || char === '[' || char === '{') {
      depth += 1;
      continue;
    }
    if (char === ')' || char === ']' || char === '}') {
      depth -= 1;
      if (depth === 0) {
        const last = source.slice(start, index).trim();
        if (last) {
          args.push(last);
        }
        return { args, end: index };
      }
      continue;
    }
    if (char === ',' && depth === 1) {
      args.push(source.slice(start, index).trim());
      start = index + 1;
    }
  }
  throw new Error('Unclosed effect call.');
}

function parseClosure(argument: string): string {
  let body = argument.trim();
  if (body.startsWith('move')) {
    body = body.slice(4).trim();
  }
  if (!body.startsWith('|')) {
    throw new Error('An effect callback must be a closure.');
  }
  const close = body.indexOf('|', 1);
  if (close === -1) {
    throw new Error('An effect callback must be a closure.');
  }
  if (body.slice(1, close).trim() !== '') {
    throw new Error('Effect closures take captured bindings, not parameters.');
  }
  return body.slice(close + 1).trim();
}

function stringLiteral(argument: string): string | undefined {
  const match = argument.match(/^"((?:\\.|[^"\\])*)"$/);
  if (!match?.[1]) {
    return undefined;
  }
  return match[1].replace(/\\"/g, '"').replace(/\\\\/g, '\\');
}

function declaredInBody(body: string): Set<string> {
  const names = new Set<string>();
  const pattern = /\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)/g;
  for (const match of body.matchAll(pattern)) {
    if (match[1]) {
      names.add(match[1]);
    }
  }
  return names;
}

function capturesIn(body: string): string[] {
  const local = declaredInBody(body);
  const found: string[] = [];
  const seen = new Set<string>();
  for (let index = 0; index < body.length; index += 1) {
    const ident = readIdent(body, index);
    if (!ident) {
      continue;
    }
    const previous = body[index - 1];
    const after = skipSpace(body, index + ident.length);
    const next = body[after];
    const call = next === '(';
    const path = next === ':';
    const field = previous === '.';
    if (
      !field &&
      !call &&
      !path &&
      !RUST_KEYWORDS.has(ident) &&
      !local.has(ident) &&
      !seen.has(ident)
    ) {
      seen.add(ident);
      found.push(ident);
    }
    index += ident.length - 1;
  }
  return found;
}

function captureType(source: string, name: string, before: number): RustCapture['ty'] {
  const prelude = source.slice(0, before);
  const pattern = new RegExp(
    String.raw`(?:let\s+(?:mut\s+)?|[(,]\s*)${name}\s*:\s*([A-Za-z0-9_:<>,\s]+)`,
    'g',
  );
  let ty: string | undefined;
  for (const match of prelude.matchAll(pattern)) {
    ty = match[1]?.trim();
  }
  if (ty === 'std::string::String') {
    ty = 'String';
  }
  if (ty === 'f64' || ty === 'bool' || ty === 'String') {
    return ty;
  }
  throw new Error(
    ty
      ? `Effect capture \`${name}\` has type \`${ty}\`, which the harness cannot bind. Use f64, bool, or String.`
      : `Effect capture \`${name}\` needs a type ascription (\`let ${name}: f64\`, \`bool\`, or \`String\`).`,
  );
}

export function extractRustEffects(source: string): RustEffectClosure[] {
  const masked = maskCommentsAndStrings(source);
  const aliases = effectAliases(masked);
  const effects: RustEffectClosure[] = [];
  const seen = new Set<string>();

  for (let index = 0; index < masked.length; index += 1) {
    const ident = readIdent(masked, index);
    if (!ident || !aliases.has(ident) || isIdentChar(masked[index - 1])) {
      if (ident) {
        index += ident.length - 1;
      }
      continue;
    }
    const open = skipSpace(masked, index + ident.length);
    if (masked[open] !== '(') {
      index += ident.length - 1;
      continue;
    }
    const call = splitArguments(source, open);
    const key = call.args[0] ? stringLiteral(call.args[0]) : undefined;
    if (!key || !call.args[1]) {
      index = call.end;
      continue;
    }
    if (seen.has(key)) {
      throw new Error(`Duplicate effect key \`${key}\`.`);
    }
    seen.add(key);
    const body = parseClosure(call.args[1]);
    const captures = capturesIn(body).map((name) => ({
      name,
      ty: captureType(source, name, open),
    }));
    effects.push({ key, body, captures });
    index = call.end;
  }

  return effects;
}

function rustString(value: string): string {
  return JSON.stringify(value);
}

function binder(capture: RustCapture): string {
  if (capture.ty === 'f64') {
    return `let ${capture.name}: f64 = require_f64(input, ${rustString(capture.name)})?;`;
  }
  if (capture.ty === 'bool') {
    return `let ${capture.name}: bool = require_bool(input, ${rustString(capture.name)})?;`;
  }
  return `let ${capture.name}: String = require_string(input, ${rustString(capture.name)})?;`;
}

export function rustHarnessSources(
  effects: RustEffectClosure[],
  dependencies: string,
): RustHarnessFiles {
  const functions = effects
    .map((effect, index) => {
      const bindings = effect.captures.map((capture) => `    ${binder(capture)}`).join('\n');
      return `fn effect_${index}(input: &serde_json::Value) -> Result<serde_json::Value, String> {
${bindings}
    let value = { ${effect.body} };
    serde_json::to_value(value).map_err(|error| error.to_string())
}`;
    })
    .join('\n\n');
  const arms = effects
    .map((effect, index) => `        ${rustString(effect.key)} => effect_${index}(&input),`)
    .join('\n');

  const handlersRs = `use serde_json::Value;

fn require_f64(input: &Value, name: &str) -> Result<f64, String> {
    input.get(name).and_then(Value::as_f64).ok_or_else(|| format!("missing {name}"))
}

fn require_bool(input: &Value, name: &str) -> Result<bool, String> {
    input.get(name).and_then(Value::as_bool).ok_or_else(|| format!("missing {name}"))
}

fn require_string(input: &Value, name: &str) -> Result<String, String> {
    input
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing {name}"))
}

${functions}

pub fn dispatch(key: &str, input: &Value) -> Result<Value, String> {
    match key {
${arms}
        _ => Err(format!("unknown effect {key}")),
    }
}
`;

  const mainRs = `mod handlers;

use serde_json::Value;

fn main() {
    let request: Value = serde_json::from_reader(std::io::stdin()).unwrap_or(serde_json::json!({}));
    let key = request.get("key").and_then(Value::as_str).unwrap_or("");
    let input = request.get("input").cloned().unwrap_or(serde_json::json!({}));
    match handlers::dispatch(key, &input) {
        Ok(value) => println!("{}", serde_json::json!({"result": value})),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
`;

  const libRs = `mod handlers;

static mut LAST_OUT: *mut u8 = std::ptr::null_mut();
static mut LAST_OUT_LEN: usize = 0;
static mut LAST_OUT_CAP: usize = 0;
static mut RESPONSE_LEN: usize = 0;
static mut ALLOC_PTR: *mut u8 = std::ptr::null_mut();
static mut ALLOC_CAP: usize = 0;

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    unsafe { buf.set_len(len); }
    let ptr = buf.as_mut_ptr();
    unsafe {
        ALLOC_PTR = ptr;
        ALLOC_CAP = buf.capacity();
    }
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn out_len() -> usize {
    unsafe { RESPONSE_LEN }
}

#[no_mangle]
pub extern "C" fn handle(ptr: *mut u8, len: usize) -> *mut u8 {
    let cap = unsafe { if ptr == ALLOC_PTR { ALLOC_CAP } else { len } };
    let input = unsafe { Vec::from_raw_parts(ptr, len, cap) };
    let response = match serde_json::from_slice::<serde_json::Value>(&input) {
        Ok(request) => {
            let key = request.get("key").and_then(serde_json::Value::as_str);
            let payload = request.get("input");
            match (key, payload) {
                (Some(key), Some(payload)) => match handlers::dispatch(key, payload) {
                    Ok(value) => serde_json::json!({ "result": value }),
                    Err(message) => serde_json::json!({ "error": message }),
                },
                _ => serde_json::json!({ "error": "effect request requires key and input" }),
            }
        }
        Err(error) => serde_json::json!({ "error": error.to_string() }),
    };
    let bytes = serde_json::to_vec(&response).unwrap_or_else(|error| {
        serde_json::to_vec(&serde_json::json!({ "error": error.to_string() })).unwrap()
    });
    remember_response(bytes)
}

fn remember_response(bytes: Vec<u8>) -> *mut u8 {
    unsafe {
        if !LAST_OUT.is_null() {
            drop(Vec::from_raw_parts(LAST_OUT, LAST_OUT_LEN, LAST_OUT_CAP));
        }
    }
    let ptr = bytes.as_ptr() as *mut u8;
    unsafe {
        LAST_OUT = ptr;
        LAST_OUT_LEN = bytes.len();
        LAST_OUT_CAP = bytes.capacity();
        RESPONSE_LEN = bytes.len();
    }
    std::mem::forget(bytes);
    ptr
}
`;

  const cargoToml = `[package]
name = "trigora-effects"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]
path = "src/lib.rs"

[[bin]]
name = "trigora-effects"
path = "src/main.rs"

[profile.release]
lto = true
opt-level = "s"
panic = "abort"

[dependencies]
serde_json = "1"
${dependencies}
`;

  return { cargoToml, mainRs, libRs, handlersRs };
}

function isInside(parent: string, child: string): boolean {
  const relative = path.relative(parent, child);
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

export function findCargoToml(startDir: string, rootDir: string): string | undefined {
  let current = path.resolve(startDir);
  const root = path.resolve(rootDir);
  while (isInside(root, current)) {
    const candidate = path.join(current, 'Cargo.toml');
    if (fs.existsSync(candidate)) {
      return candidate;
    }
    const parent = path.dirname(current);
    if (parent === current) {
      break;
    }
    current = parent;
  }
  return undefined;
}

export function copiedDependencies(cargoToml: string, fromDir: string, harnessDir: string): string {
  const lines = cargoToml.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === '[dependencies]');
  if (start === -1) {
    return '';
  }
  const copied: string[] = [];
  for (const line of lines.slice(start + 1)) {
    if (/^\s*\[/.test(line)) {
      break;
    }
    if (!line.trim() || line.trim().startsWith('#')) {
      continue;
    }
    if (/^\s*(trigora|tcc-rust-prelude|serde_json)\b/.test(line)) {
      continue;
    }
    copied.push(
      line.replace(/path\s*=\s*"([^"]+)"/g, (_match, relative: string) => {
        const absolute = path.resolve(fromDir, relative);
        let next = path.relative(harnessDir, absolute);
        if (!next.startsWith('.')) {
          next = `./${next}`;
        }
        return `path = "${next.split(path.sep).join('/')}"`;
      }),
    );
  }
  return copied.length === 0 ? '' : `${copied.join('\n')}\n`;
}

function writeHarness(harnessDir: string, files: RustHarnessFiles): void {
  fs.mkdirSync(path.join(harnessDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(harnessDir, 'Cargo.toml'), files.cargoToml);
  fs.writeFileSync(path.join(harnessDir, 'src', 'main.rs'), files.mainRs);
  fs.writeFileSync(path.join(harnessDir, 'src', 'lib.rs'), files.libRs);
  fs.writeFileSync(path.join(harnessDir, 'src', 'handlers.rs'), files.handlersRs);
}

export function rustEffectWasm(options: {
  rootDir: string;
  programId: string;
  file: string;
  source: string;
}): Buffer {
  const effects = extractRustEffects(options.source);
  const harnessDir = path.join(options.rootDir, '.trigora', 'effects', options.programId);
  const cargoFile = findCargoToml(
    path.dirname(path.join(options.rootDir, options.file)),
    options.rootDir,
  );
  const dependencies = cargoFile
    ? copiedDependencies(fs.readFileSync(cargoFile, 'utf8'), path.dirname(cargoFile), harnessDir)
    : '';
  const files = rustHarnessSources(effects, dependencies);
  writeHarness(harnessDir, files);
  const built = spawnSync(
    'cargo',
    [
      'build',
      '--quiet',
      '--lib',
      '--release',
      '--target',
      'wasm32-unknown-unknown',
      '--manifest-path',
      path.join(harnessDir, 'Cargo.toml'),
      '--target-dir',
      path.join(harnessDir, 'target'),
    ],
    { encoding: 'utf8' },
  );
  if (built.status !== 0) {
    throw new CliDisplayError({
      title: 'Effect worker failed to build',
      details: [
        {
          label: 'Reason',
          value: (
            built.stderr ||
            built.stdout ||
            built.error?.message ||
            'cargo build failed'
          ).trim(),
        },
        { label: 'Program', value: options.programId },
      ],
      hint: 'Install the wasm32-unknown-unknown target with rustup, then retry.',
    });
  }
  return fs.readFileSync(
    path.join(harnessDir, 'target', 'wasm32-unknown-unknown', 'release', 'trigora_effects.wasm'),
  );
}

export function rustEffectBinary(options: {
  rootDir: string;
  programId: string;
  file: string;
  source: string;
}): string {
  const effects = extractRustEffects(options.source);
  const harnessDir = path.join(options.rootDir, '.trigora', 'effects', options.programId);
  const cargoFile = findCargoToml(
    path.dirname(path.join(options.rootDir, options.file)),
    options.rootDir,
  );
  const dependencies = cargoFile
    ? copiedDependencies(fs.readFileSync(cargoFile, 'utf8'), path.dirname(cargoFile), harnessDir)
    : '';
  const files = rustHarnessSources(effects, dependencies);
  const stamp = createHash('sha256')
    .update(files.cargoToml)
    .update(files.mainRs)
    .update(files.libRs)
    .update(files.handlersRs)
    .digest('hex');
  const binary = path.join(
    harnessDir,
    'target',
    'debug',
    process.platform === 'win32' ? 'trigora-effects.exe' : 'trigora-effects',
  );
  const stampPath = path.join(harnessDir, '.built');
  if (
    fs.existsSync(binary) &&
    fs.existsSync(stampPath) &&
    fs.readFileSync(stampPath, 'utf8') === stamp
  ) {
    return binary;
  }

  writeHarness(harnessDir, files);
  const built = spawnSync(
    'cargo',
    [
      'build',
      '--quiet',
      '--bin',
      'trigora-effects',
      '--manifest-path',
      path.join(harnessDir, 'Cargo.toml'),
      '--target-dir',
      path.join(harnessDir, 'target'),
    ],
    { encoding: 'utf8' },
  );
  if (built.status !== 0) {
    throw new CliDisplayError({
      title: 'Effect harness failed to build',
      details: [
        {
          label: 'Reason',
          value: (
            built.stderr ||
            built.stdout ||
            built.error?.message ||
            'cargo build failed'
          ).trim(),
        },
        { label: 'Program', value: options.programId },
      ],
    });
  }
  fs.writeFileSync(stampPath, stamp);
  return binary;
}

export function runRustEffect(binary: string, key: string, input: unknown): unknown {
  const child = spawnSync(binary, [], {
    input: JSON.stringify({ key, input: input ?? {} }),
    encoding: 'utf8',
  });
  if (child.error) {
    throw new Error(child.error.message);
  }
  if (child.status !== 0) {
    throw new Error(child.stderr.trim() || child.stdout.trim() || `Effect \`${key}\` failed.`);
  }
  const parsed = JSON.parse(child.stdout) as { result?: unknown; error?: string };
  if (parsed.error) {
    throw new Error(parsed.error);
  }
  return parsed.result;
}
