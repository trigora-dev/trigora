import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

import { analyzeSource } from './analyzeSource';
import { publish, searchWeb } from './tools';

export type ResearchInput = {
  query: string;
};

export const approved = event<{ reviewer: string }>('approved');

export async function researchAgent(input: ResearchInput) {
  const sources = await effect('search', () => searchWeb(input.query));

  const reports = await Promise.all(sources.map((source) => invoke(analyzeSource, { source })));

  const approval = await waitForEvent(approved);

  return effect('publish', () =>
    publish({
      reports,
      reviewer: approval.reviewer,
    }),
  );
}
