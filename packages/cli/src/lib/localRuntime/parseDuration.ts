const UNIT_MS: Record<string, number> = {
  ms: 1,
  s: 1000,
  m: 60_000,
  h: 3_600_000,
  d: 86_400_000,
};

export function parseDuration(input: string | number, label = 'duration'): number {
  if (typeof input === 'number') {
    if (!Number.isFinite(input) || input < 0) {
      throw new Error(`${label} must be a non-negative number of milliseconds.`);
    }

    return input;
  }

  const match = /^(\d+(?:\.\d+)?)(ms|s|m|h|d)$/.exec(input.trim());

  if (!match) {
    throw new Error(
      `Invalid ${label} "${input}". Use a number of milliseconds or a string like "5s", "30m", or "7d".`,
    );
  }

  const amount = Number(match[1]);
  const unit = match[2];

  if (unit === undefined) {
    throw new Error(`Invalid ${label} "${input}".`);
  }

  return amount * (UNIT_MS[unit] ?? 1);
}
