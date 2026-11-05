import { effect } from '@trigora/sdk';

export default async function other() {
  return await effect('other', () => 0);
}
