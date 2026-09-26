import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(cliRoot, '../..');
const extension = process.platform === 'win32' ? '.exe' : '';
const built = path.join(repoRoot, 'target', 'release', `trigora-local${extension}`);
const slot = path.join(cliRoot, 'vendor', 'trigora-local', `${process.platform}-${process.arch}`);
const destination = path.join(slot, `trigora-local${extension}`);

const cargo = spawnSync('cargo', ['build', '--release', '-p', 'trigora-local'], {
  cwd: repoRoot,
  stdio: 'inherit',
});
if (cargo.status !== 0) {
  process.exit(cargo.status ?? 1);
}

fs.mkdirSync(slot, { recursive: true });
fs.copyFileSync(built, destination);
fs.chmodSync(destination, 0o755);
