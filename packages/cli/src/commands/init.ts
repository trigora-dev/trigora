import fs from 'node:fs/promises';
import path from 'node:path';

import { colors } from '../lib/colors';

type InitOptions = {
  force?: boolean;
};

const HELLO_FLOW_TEMPLATE = `import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', {
      payload: event.payload,
    });
  },
});
`;

const PAYLOAD_TEMPLATE = `{
  "message": "Hello, world!"
}
`;

const ENV_EXAMPLE_TEMPLATE = `# Add environment variables here
`;

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
): Promise<void> {
  const exists = await pathExists(filePath);
  const relative = path.relative(process.cwd(), filePath);
  const initPrefix = colors.dev('[init]');

  if (exists && !options.force) {
    console.log(`${initPrefix} ${colors.warn(`Skipped ${relative} (already exists)`)}`);
    return;
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, contents, 'utf-8');

  if (exists && options.force) {
    console.log(`${initPrefix} ${colors.warn(`Updated ${relative}`)}`);
    return;
  }

  console.log(`${initPrefix} ${colors.success(`Created ${relative}`)}`);
}

export async function initCommand(options: InitOptions): Promise<void> {
  const cwd = process.cwd();
  const initPrefix = colors.dev('[init]');

  await writeFile(path.join(cwd, 'flows', 'hello.ts'), HELLO_FLOW_TEMPLATE, options);
  await writeFile(path.join(cwd, 'payload.json'), PAYLOAD_TEMPLATE, options);
  await writeFile(path.join(cwd, '.env.example'), ENV_EXAMPLE_TEMPLATE, options);

  console.log('');
  console.log(`${initPrefix} ${colors.flow('Next steps:')}`);
  console.log(`${initPrefix} trigora trigger hello --payload payload.json`);
  console.log(`${initPrefix} trigora dev hello --payload payload.json`);
}