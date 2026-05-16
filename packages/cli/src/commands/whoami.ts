import type { WhoAmIResponse } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { getDeployToken } from '../lib/getDeployToken';
import {
  printWhoAmI,
  toWhoAmIApiFailure,
  toWhoAmITokenFailure,
  whoAmISteps,
} from '../lib/whoamiOutput';

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toWhoAmITokenFailure();
  }

  return token;
}

function createWhoAmIApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

export async function whoAmICommand(): Promise<WhoAmIResponse> {
  const identity = await createWhoAmIApiClient()
    .whoAmI()
    .catch((error) => {
      throw toWhoAmIApiFailure(error, whoAmISteps.fetchingIdentity);
    });

  printWhoAmI(identity);

  return identity;
}
