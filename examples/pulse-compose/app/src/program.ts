import { appendFileSync } from 'node:fs';

import { effect, waitForEvent } from '@trigora/sdk';

export default async function program() {
  const first = await effect('a', () => {
    appendFileSync(process.env.PULSE_EFFECT_LOG ?? '', 'a 1\n');
    return 1;
  });
  await waitForEvent('continue');
  const second = await effect('b', () => {
    appendFileSync(process.env.PULSE_EFFECT_LOG ?? '', 'b 2\n');
    return 2;
  });
  return { first, second };
}
