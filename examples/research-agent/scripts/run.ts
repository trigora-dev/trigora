import { createClient } from '@trigora/client';

import { approved, researchAgent } from '../src/researchAgent';

const client = createClient();

const run = await client.start(researchAgent, {
  query: 'durable agent infrastructure',
});

console.log(`started ${run.id}`);
console.log('waiting for approval; sending it now...');

await run.send(approved, {
  reviewer: 'Omar',
});

const report = await run.result();
console.log(report);
