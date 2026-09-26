import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(cliRoot, '../..');
const extension = process.platform === 'win32' ? '.exe' : '';
const platform = `${process.platform}-${process.arch}`;

function cargo(args, cwd) {
  const result = spawnSync('cargo', args, { cwd, stdio: 'inherit' });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function install(source, slotName, binaryName) {
  const destination = path.join(cliRoot, 'vendor', slotName, platform, `${binaryName}${extension}`);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(source, destination);
  fs.chmodSync(destination, 0o755);
}

cargo(['build', '--release', '-p', 'trigora-cli'], repoRoot);
install(path.join(repoRoot, 'target', 'release', `trigora${extension}`), 'trigora', 'trigora');
install(
  path.join(repoRoot, 'target', 'release', `tcc-rust-compile${extension}`),
  'tcc-rust-compile',
  'tcc-rust-compile',
);
