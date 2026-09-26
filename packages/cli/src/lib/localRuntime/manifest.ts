import { parse } from 'smol-toml';

import type { JsonValue, TriggerConfig } from '@trigora/contracts';

export type SchemaManifest = {
  projectName: string;
  programs: string[];
  triggers: TriggerConfig[];
};

const TOP_LEVEL = new Set(['project', 'triggers']);
const PROJECT_KEYS = new Set(['name', 'programs']);
const TRIGGER_KEYS = new Set(['name', 'type', 'program', 'schedule', 'timezone', 'input']);

const CRON_BOUNDS: Array<[number, number]> = [
  [0, 59],
  [0, 23],
  [1, 31],
  [1, 12],
  [0, 7],
];

function validateCron(schedule: string, name: string): void {
  const fields = schedule.trim().split(/\s+/);
  if (fields.length !== 5) {
    throw new Error(`Cron trigger "${name}" schedule must have five fields.`);
  }
  fields.forEach((field, index) => {
    const bounds = CRON_BOUNDS[index];
    if (!bounds) {
      return;
    }
    validateCronField(field, bounds[0], bounds[1], name);
  });
}

function validateCronField(field: string, min: number, max: number, name: string): void {
  if (!field) {
    throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
  }
  for (const listItem of field.split(',')) {
    if (!listItem) {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
    const [range, step, extra] = listItem.split('/');
    if (extra !== undefined || range === undefined || range === '') {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
    if (step !== undefined && (!/^\d+$/.test(step) || Number(step) < 1)) {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
    if (range === '*') {
      continue;
    }
    const ends = range.split('-');
    if (ends.length > 2 || ends.some((end) => !/^\d+$/.test(end))) {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
    const numbers = ends.map((end) => Number(end));
    if (numbers.some((value) => value < min || value > max)) {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
    if (numbers.length === 2 && numbers[0]! > numbers[1]!) {
      throw new Error(`Cron trigger "${name}" has an invalid schedule.`);
    }
  }
}

function validTimezone(timezone: string): boolean {
  if (timezone === 'UTC') {
    return true;
  }
  return Intl.supportedValuesOf('timeZone').includes(timezone);
}

function rejectDates(value: unknown, path: string): JsonValue {
  if (value instanceof Date) {
    throw new Error(`TOML datetimes are not allowed at ${path}.`);
  }
  if (value === null) {
    throw new Error(`null is not allowed at ${path}.`);
  }
  if (typeof value === 'string' || typeof value === 'boolean') {
    return value;
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) {
      throw new Error(`Invalid number at ${path}.`);
    }
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item, index) => rejectDates(item, `${path}[${index}]`));
  }
  if (typeof value === 'object') {
    const record: Record<string, JsonValue> = {};
    for (const [key, item] of Object.entries(value as Record<string, unknown>)) {
      record[key] = rejectDates(item, `${path}.${key}`);
    }
    return record;
  }
  throw new Error(`Unsupported value at ${path}.`);
}

function unknownKeys(value: Record<string, unknown>, allowed: Set<string>, label: string): void {
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw new Error(`Unknown field \`${key}\` in ${label}.`);
    }
  }
}

export function parseSchemaManifest(source: string): SchemaManifest {
  const parsed = parse(source) as Record<string, unknown>;
  unknownKeys(parsed, TOP_LEVEL, 'trigora.toml');
  const project = parsed.project;
  if (typeof project !== 'object' || project === null || Array.isArray(project)) {
    throw new Error('`[project].name` is required.');
  }
  unknownKeys(project as Record<string, unknown>, PROJECT_KEYS, '[project]');
  const projectName = (project as { name?: unknown }).name;
  if (typeof projectName !== 'string' || !projectName.trim()) {
    throw new Error('`[project].name` is required.');
  }

  const programs = readProgramGlobs((project as { programs?: unknown }).programs);
  if (programs.length === 0) {
    throw new Error('`[project].programs` must be a glob or a list of globs.');
  }

  const rawTriggers = parsed.triggers ?? [];
  if (!Array.isArray(rawTriggers)) {
    throw new Error('`triggers` must be an array of tables.');
  }
  const triggers = rawTriggers.map((entry, index) => decodeTrigger(entry, index));
  const names = new Set<string>();
  for (const trigger of triggers) {
    if (names.has(trigger.name)) {
      throw new Error(`Trigger name "${trigger.name}" is duplicated.`);
    }
    names.add(trigger.name);
  }

  return {
    projectName: projectName.trim(),
    programs: programs.map((item) => item.trim()),
    triggers,
  };
}

function readProgramGlobs(value: unknown): string[] {
  const items = typeof value === 'string' ? [value] : value;
  if (!Array.isArray(items) || items.some((item) => typeof item !== 'string' || !item.trim())) {
    throw new Error('`[project].programs` must be a glob or a list of globs.');
  }
  return items.map((item) => item.trim());
}

function decodeTrigger(entry: unknown, index: number): TriggerConfig {
  if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
    throw new Error(`Trigger ${index + 1} must be a table.`);
  }
  const row = entry as Record<string, unknown>;
  unknownKeys(row, TRIGGER_KEYS, `triggers[${index}]`);
  const name = row.name;
  const type = row.type;
  const program = row.program;
  if (typeof name !== 'string' || !name.trim()) {
    throw new Error(`Trigger ${index + 1} requires a name.`);
  }
  if (type !== 'webhook' && type !== 'cron') {
    throw new Error(`Trigger "${name}" type must be webhook or cron.`);
  }
  if (typeof program !== 'string' || !program.trim()) {
    throw new Error(`Trigger "${name}" requires a program.`);
  }
  if ('path' in row) {
    throw new Error(`Trigger "${name}" cannot set path.`);
  }
  const input = row.input === undefined ? undefined : rejectDates(row.input, `${name}.input`);
  if (type === 'webhook') {
    if (row.schedule !== undefined || row.timezone !== undefined || input !== undefined) {
      throw new Error(`Webhook trigger "${name}" cannot set schedule, timezone, or input.`);
    }
    return {
      name: name.trim(),
      type,
      program: program.trim(),
    };
  }
  if (typeof row.schedule !== 'string' || !row.schedule.trim()) {
    throw new Error(`Cron trigger "${name}" requires a schedule.`);
  }
  validateCron(row.schedule, name.trim());
  const timezone = row.timezone === undefined ? 'UTC' : row.timezone;
  if (typeof timezone !== 'string' || !timezone.trim() || !validTimezone(timezone.trim())) {
    throw new Error(`Cron trigger "${name}" has an invalid timezone.`);
  }
  return {
    name: name.trim(),
    type,
    program: program.trim(),
    schedule: row.schedule.trim(),
    timezone: timezone.trim(),
    ...(input === undefined ? {} : { input }),
  };
}
