import fs from 'node:fs/promises';
import path from 'node:path';

import { colors } from '../lib/colors';

type InitOptions = {
  force?: boolean;
};

const HELLO_FLOW_TEMPLATE = `import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'webhook' },
  async run(event, ctx) {
    await ctx.log.info('Received event', event.payload);
  },
});
`;

const PAYLOAD_TEMPLATE = `{
  "message": "Hello, world!"
}
`;

const ENV_EXAMPLE_TEMPLATE = `# Trigora Cloud
TRIGORA_DEPLOY_TOKEN=your-deploy-token
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
    writeFile(path.join(cwd, 'flows', 'hello.ts'), HELLO_FLOW_TEMPLATE, options),
    writeFile(path.join(cwd, 'payload.json'), PAYLOAD_TEMPLATE, options),
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
  console.log(`  ${colors.label('1.')} trigora dev hello`);
  console.log(`  ${colors.label('2.')} trigora trigger hello --payload payload.json`);
  console.log(`  ${colors.label('3.')} trigora deploy hello`);
}
