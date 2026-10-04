#!/usr/bin/env node
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const platform = `${process.platform}-${process.arch}`;
const extension = process.platform === 'win32' ? '.exe' : '';

function vendor(name) {
  return path.join(root, 'vendor', name, platform, `${name}${extension}`);
}

const binary =
  process.env.TRIGORA_BIN || path.join(root, 'vendor', 'trigora', platform, `trigora${extension}`);
if (!fs.existsSync(binary)) {
  console.error('The trigora binary is missing. Reinstall the CLI with `npm install -g trigora`.');
  process.exit(1);
}

process.env.TRIGORA_NODE_HELPER ??= path.join(root, 'helper', 'node-helper.js');
process.env.TRIGORA_RUST_COMPILER_BIN ??= vendor('tcc-rust-compile');
process.env.TRIGORA_LOCAL_BIN ??= vendor('trigora-local');
const nodeModules = path.join(root, 'node_modules');
process.env.NODE_PATH = [nodeModules, process.env.NODE_PATH].filter(Boolean).join(path.delimiter);

const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', env: process.env });
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    child.kill(signal);
  });
}
child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
child.on('error', (error) => {
  console.error(error.message);
  process.exit(1);
});
