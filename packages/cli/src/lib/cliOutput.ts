import { colors } from './colors';

export type CliDetail = {
  label: string;
  value: string;
};

export type CliSection = {
  title: string;
  lines: string[];
};

type CliDisplayErrorConfig = {
  title: string;
  details: CliDetail[];
  hint?: string;
  message?: string;
};

export class CliDisplayError extends Error {
  readonly details: CliDetail[];
  readonly hint?: string;
  readonly title: string;

  constructor(config: CliDisplayErrorConfig) {
    super(config.message ?? config.details[0]?.value ?? config.title);
    this.name = 'CliDisplayError';
    this.title = config.title;
    this.details = config.details;
    this.hint = config.hint;
  }
}

export function isCliDisplayError(error: unknown): error is CliDisplayError {
  return error instanceof CliDisplayError;
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return count === 1 ? singular : plural;
}

export function printProgress(scope: string, message: string): void {
  console.log(`${colors.dev(`[${scope}]`)} ${message}`);
}

export function printWarning(message: string): void {
  console.warn(`${colors.warn('[warn]')} ${message}`);
}

function getLabelWidth(details: CliDetail[]): number {
  return details.reduce((width, detail) => Math.max(width, detail.label.length), 0);
}

function printDetails(details: CliDetail[], writer: (message: string) => void): void {
  if (details.length === 0) {
    return;
  }

  const labelWidth = getLabelWidth(details);

  for (const detail of details) {
    writer(`  ${colors.label(detail.label.padEnd(labelWidth))}  ${detail.value}`);
  }
}

function printSections(sections: CliSection[], writer: (message: string) => void): void {
  for (const section of sections) {
    if (section.lines.length === 0) {
      continue;
    }

    writer('');
    writer(`  ${colors.heading(section.title)}`);

    for (const line of section.lines) {
      writer(`  ${line}`);
    }
  }
}

export function printSuccessSummary(
  title: string,
  details: CliDetail[],
  sections: CliSection[] = [],
  footer?: string,
): void {
  console.log('');
  console.log(colors.success(`✔ ${title}`));

  if (details.length > 0 || sections.length > 0 || footer) {
    console.log('');
  }

  printDetails(details, console.log);
  printSections(sections, console.log);

  if (footer) {
    console.log('');
    console.log(`  ${footer}`);
  }
}

export function renderCliError(error: CliDisplayError): void {
  console.error('');
  console.error(colors.error(`✖ ${error.title}`));

  if (error.details.length > 0 || error.hint) {
    console.error('');
  }

  printDetails(error.details, console.error);

  if (error.hint) {
    console.error('');
    console.error(`  ${error.hint}`);
  }
}
