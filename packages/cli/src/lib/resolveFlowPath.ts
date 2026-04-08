import fs from 'node:fs';
import path from 'node:path';

export function resolveFlowPath(flowNameOrPath: string): string {
  const cwd = process.cwd();

  const directPath = path.resolve(cwd, flowNameOrPath);
  if (fs.existsSync(directPath)) {
    return directPath;
  }

  const candidates = [
    path.join(cwd, 'flows', `${flowNameOrPath}.ts`),
    path.join(cwd, 'flows', `${flowNameOrPath}.js`),
    path.join(cwd, `${flowNameOrPath}.ts`),
    path.join(cwd, `${flowNameOrPath}.js`),
  ];

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  throw new Error(`Could not find flow "${flowNameOrPath}".`);
}
