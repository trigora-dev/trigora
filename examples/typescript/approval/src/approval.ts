import { effect, waitForEvent } from '@trigora/sdk';

export default async function approval() {
  const result = await effect('generate', () => 42);
  const review = await waitForEvent('approved');
  return { result, review };
}
