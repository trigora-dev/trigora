export type Source = string;

export type Report = {
  source: Source;
  summary: string;
};

export function searchWeb(query: string): Source[] {
  return [
    `https://example.com/${encodeURIComponent(query)}`,
    `https://docs.example.com/${encodeURIComponent(query)}`,
  ];
}

export function publish(input: { reports: Report[]; reviewer: string }) {
  return {
    published: true,
    reviewer: input.reviewer,
    reportCount: input.reports.length,
    titles: input.reports.map((report) => report.summary),
  };
}
