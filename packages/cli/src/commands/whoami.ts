import type { WhoAmIResponse } from '@trigora/contracts';

import { requireCloudClient } from '../lib/cloudRuntime';
import { printWhoAmI, toWhoAmIApiFailure, whoAmISteps } from '../lib/whoamiOutput';

export async function whoAmICommand(): Promise<WhoAmIResponse> {
  const client = requireCloudClient();
  try {
    const identity = await client.whoAmI();
    printWhoAmI(identity);
    return identity;
  } catch (error) {
    throw toWhoAmIApiFailure(error, whoAmISteps.fetchingIdentity);
  }
}
