import { effect } from '@trigora/sdk';

import type { Report, Source } from './tools';

export async function analyzeSource(input: { source: Source }): Promise<Report> {
  return effect('analyze', () => ({
    source: input.source,
    summary: `analysis of ${input.source}`,
  }));
}
