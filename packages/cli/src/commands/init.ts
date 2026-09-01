import fs from 'node:fs/promises';
import path from 'node:path';

import { colors } from '../lib/colors';

type InitOptions = {
  force?: boolean;
};

const CONFIG_TEMPLATE = `import { defineConfig } from '@trigora/sdk';

export default defineConfig({
  programs: './src/programs/**/*.ts',
});
`;

const HELLO_PROGRAM_TEMPLATE = `import { effect, event, waitForEvent } from '@trigora/sdk';

export const greeted = event<{ name: string }>('greeted');

export async function hello(input: { query: string }) {
  const greeting = await effect('greet', () => \`hello \${input.query}\`);
  const who = await waitForEvent(greeted);

  return {
    greeting,
    from: who.name,
  };
}
`;

const ENV_EXAMPLE_TEMPLATE = `# Local runtime used by @trigora/client while \`trigora dev\` is running.
TRIGORA_RUNTIME_URL=http://127.0.0.1:3477
`;

type FileWriteResult = {
  path: string;
  status: 'created' | 'skipped' | 'updated';
};

async function pathExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function writeFile(
  filePath: string,
  contents: string,
  options: InitOptions,
): Promise<FileWriteResult> {
  const exists = await pathExists(filePath);
  const relative = path.relative(process.cwd(), filePath);

  if (exists && !options.force) {
    return {
      path: relative,
      status: 'skipped',
    };
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, contents, 'utf-8');

  if (exists && options.force) {
    return {
      path: relative,
      status: 'updated',
    };
  }

  return {
    path: relative,
    status: 'created',
  };
}

function printFileGroup(title: string, colorize: (value: string) => string, paths: string[]): void {
  if (paths.length === 0) {
    return;
  }

  console.log(colorize(title));
  for (const filePath of paths) {
    console.log(`  ${filePath}`);
  }
  console.log('');
}

export async function initCommand(options: InitOptions): Promise<void> {
  const cwd = process.cwd();
  const results = await Promise.all([
    writeFile(path.join(cwd, 'trigora.config.ts'), CONFIG_TEMPLATE, options),
    writeFile(path.join(cwd, 'src', 'programs', 'hello.ts'), HELLO_PROGRAM_TEMPLATE, options),
    writeFile(path.join(cwd, '.env.example'), ENV_EXAMPLE_TEMPLATE, options),
  ]);
  const created = results
    .filter((result) => result.status === 'created')
    .map((result) => result.path);
  const updated = results
    .filter((result) => result.status === 'updated')
    .map((result) => result.path);
  const skipped = results
    .filter((result) => result.status === 'skipped')
    .map((result) => result.path);

  console.log('');
  console.log(`${colors.success('✔')} Project initialized`);
  console.log('');

  printFileGroup('Created', colors.success, created);
  printFileGroup('Updated', colors.warn, updated);
  printFileGroup('Skipped', colors.warn, skipped);

  console.log(colors.heading('Next steps'));
  console.log(`  ${colors.label('1.')} trigora dev`);
  console.log(
    `  ${colors.label('2.')} start hello with @trigora/client, then send the greeted event`,
  );
}
