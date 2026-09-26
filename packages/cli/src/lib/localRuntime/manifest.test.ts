import { describe, expect, it } from 'vitest';

import { parseSchemaManifest } from './manifest';

const PROJECT = `[project]
name = "reports"
programs = "src/**/*.py"
`;

describe('parseSchemaManifest', () => {
  it('parses a webhook trigger', () => {
    const manifest = parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "github"
type = "webhook"
program = "report"
`);
    expect(manifest.triggers).toEqual([{ name: 'github', type: 'webhook', program: 'report' }]);
  });

  it('parses a cron trigger and defaults timezone to UTC', () => {
    const manifest = parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "nightly"
type = "cron"
program = "report"
schedule = "0 2 * * *"
`);
    expect(manifest.triggers[0]).toMatchObject({
      type: 'cron',
      schedule: '0 2 * * *',
      timezone: 'UTC',
    });
  });

  it('parses nested cron input', () => {
    const manifest = parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "nightly"
type = "cron"
program = "report"
schedule = "0 2 * * *"

[triggers.input]
kind = "daily"
options = { include_archived = false }
`);
    expect(manifest.triggers[0]).toMatchObject({
      input: { kind: 'daily', options: { include_archived: false } },
    });
  });

  it('normalizes a string glob and an array to string[]', () => {
    expect(parseSchemaManifest(PROJECT).programs).toEqual(['src/**/*.py']);
    expect(
      parseSchemaManifest(`[project]
name = "reports"
programs = ["src/**/*.ts", "src/**/*.js"]
`).programs,
    ).toEqual(['src/**/*.ts', 'src/**/*.js']);
  });

  it('rejects schema and [programs].include', () => {
    expect(() => parseSchemaManifest(`schema = 1\n${PROJECT}`)).toThrow(/Unknown field `schema`/);
    expect(() =>
      parseSchemaManifest(`[project]
name = "reports"

[programs]
include = ["src/**/*.py"]
`),
    ).toThrow(/Unknown field `programs`/);
  });

  it('rejects an invalid cron schedule', () => {
    expect(() =>
      parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "nightly"
type = "cron"
program = "report"
schedule = "60 2 * * *"
`),
    ).toThrow(/invalid schedule/);
  });

  it('rejects an invalid timezone', () => {
    expect(() =>
      parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "nightly"
type = "cron"
program = "report"
schedule = "0 2 * * *"
timezone = "Not/AZone"
`),
    ).toThrow(/timezone/);
  });

  it('rejects duplicate trigger names', () => {
    expect(() =>
      parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "nightly"
type = "webhook"
program = "report"

[[triggers]]
name = "nightly"
type = "webhook"
program = "report"
`),
    ).toThrow(/duplicated/);
  });

  it('rejects webhook schedule, timezone, and input', () => {
    expect(() =>
      parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "github"
type = "webhook"
program = "report"
schedule = "0 2 * * *"
`),
    ).toThrow(/cannot set schedule, timezone, or input/);
    expect(() =>
      parseSchemaManifest(`${PROJECT}
[[triggers]]
name = "github"
type = "webhook"
program = "report"
input = "body"
`),
    ).toThrow(/cannot set schedule, timezone, or input/);
  });
});
