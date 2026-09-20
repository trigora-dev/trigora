import { createClient } from '@trigora/client';

async function approval() {
  return { result: 42, review: 'ok' };
}

const client = createClient();
const run = await client.start(approval, {});

console.log(`started ${run.id}`);
console.log('waiting for approval; sending it now...');

await run.send('approved', 'ok');
console.log(await run.result());
