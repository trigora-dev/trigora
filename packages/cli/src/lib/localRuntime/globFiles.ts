import fs from 'node:fs/promises';
import path from 'node:path';

function toPosix(filePath: string): string {
  return filePath.split(path.sep).join('/');
}

function stripDotSlash(pattern: string): string {
  return pattern.replace(/^\.\//, '');
}

function globToRegExp(pattern: string): RegExp {
  const normalized = stripDotSlash(toPosix(pattern));
  let expression = '^';

  for (let index = 0; index < normalized.length; index += 1) {
    const char = normalized[index];

    if (char === '*' && normalized[index + 1] === '*') {
      const next = normalized[index + 2];
      if (next === '/') {
        expression += '(?:.*/)?';
        index += 2;
      } else {
        expression += '.*';
        index += 1;
      }
      continue;
    }

    if (char === '*') {
      expression += '[^/]*';
      continue;
    }

    if (char === '?') {
      expression += '[^/]';
      continue;
    }

    if (char && /[.+^${}()|[\]\\]/.test(char)) {
      expression += `\\${char}`;
      continue;
    }

    expression += char;
  }

  expression += '$';
  return new RegExp(expression);
}

async function walk(directory: string): Promise<string[]> {
  const entries = await fs.readdir(directory, { withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    const fullPath = path.join(directory, entry.name);

    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'dist' || entry.name === '.git') {
        continue;
      }

      files.push(...(await walk(fullPath)));
      continue;
    }

    if (entry.isFile()) {
      files.push(fullPath);
    }
  }

  return files;
}

export async function globFiles(rootDir: string, patterns: string[]): Promise<string[]> {
  const matches = new Set<string>();
  const files = await walk(rootDir);

  for (const pattern of patterns) {
    const relativePattern = stripDotSlash(pattern);
    const hasGlob = /[*?]/.test(relativePattern);

    if (!hasGlob) {
      const absolute = path.resolve(rootDir, relativePattern);

      try {
        const stat = await fs.stat(absolute);
        if (stat.isFile()) {
          matches.add(absolute);
        }
      } catch {
        // Missing exact paths are ignored here; discovery reports empty programs.
      }

      continue;
    }

    const matcher = globToRegExp(relativePattern);

    for (const file of files) {
      const relative = toPosix(path.relative(rootDir, file));
      if (matcher.test(relative)) {
        matches.add(file);
      }
    }
  }

  return [...matches].sort();
}
