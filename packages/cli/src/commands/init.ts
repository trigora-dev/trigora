import fs from 'node:fs/promises';
import path from 'node:path';

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

async function writeFile(filePath: string, contents: string, options: InitOptions): Promise<void> {
  const exists = await pathExists(filePath);
  const relative = path.relative(process.cwd(), filePath);

  if (exists && !options.force) {
    console.log(`• Skipped ${relative} (already exists)`);
    return;
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, contents, 'utf-8');

  if (exists && options.force) {
    console.log(`↺ Updated ${relative}`);
    return;
  }

  console.log(`✓ Created ${relative}`);
}

export async function initCommand(options: InitOptions): Promise<void> {
  const cwd = process.cwd();

  await writeFile(path.join(cwd, 'flows', 'hello.ts'), HELLO_FLOW_TEMPLATE, options);
  await writeFile(path.join(cwd, 'payload.json'), PAYLOAD_TEMPLATE, options);
  await writeFile(path.join(cwd, '.env.example'), ENV_EXAMPLE_TEMPLATE, options);

  console.log('');
  console.log('Next steps:');
  console.log('  trigora dev hello --payload payload.json');
}
