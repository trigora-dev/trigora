import type { Execution, ExecutionSummary, ExecutionWait } from '@trigora/contracts';

import { languageFromFile } from './localRuntime/compiler';
import { colors } from './colors';

export function formatWait(wait?: ExecutionWait): string {
  if (!wait) {
    return '';
  }

  if (wait.type === 'event') {
    return `event:${wait.event}`;
  }

  if (wait.type === 'timer') {
    return `timer:${wait.wakeAt}`;
  }

  return `child:${wait.executionId}`;
}

export function formatProgramLanguage(file: string): string {
  return languageFromFile(file) ?? 'unknown';
}

function printTable(rows: Array<Array<{ label: string; value: string }>>): void {
  if (rows.length === 0) {
    return;
  }

  const widths = rows[0]?.map((_, index) =>
    rows.reduce((width, row) => Math.max(width, row[index]?.value.length ?? 0), 0),
  );

  console.log('');
  for (const row of rows) {
    console.log(
      row
        .map((cell, index) => {
          const padded = cell.value.padEnd(widths?.[index] ?? cell.value.length);
          return index === 0 ? colors.heading(padded) : colors.label(padded);
        })
        .join('  '),
    );
  }
}

export function printProgramsTable(
  programs: Array<{ id: string; language: string; file?: string }>,
): void {
  if (programs.length === 0) {
    console.log('');
    console.log(colors.label('No programs loaded. Is `trigora dev` running against this project?'));
    return;
  }

  printTable(
    programs.map((program) => [
      { label: 'id', value: program.id },
      { label: 'language', value: program.language },
      { label: 'file', value: program.file ?? '' },
    ]),
  );
}

export function printExecutionsTable(executions: Array<Execution | ExecutionSummary>): void {
  if (executions.length === 0) {
    console.log('');
    console.log(colors.label('No executions found.'));
    return;
  }

  printTable(
    executions.map((execution) => [
      { label: 'id', value: execution.id },
      { label: 'program', value: execution.programId },
      { label: 'status', value: execution.status },
      { label: 'wait', value: formatWait(execution.wait) },
    ]),
  );
}

export function printExecutionRecord(execution: Execution | ExecutionSummary): void {
  const details = [
    { label: 'ID', value: colors.heading(execution.id) },
    { label: 'Program', value: execution.programId },
    { label: 'Status', value: execution.status },
  ];

  const wait = formatWait(execution.wait);
  if (wait) {
    details.push({ label: 'Wait', value: wait });
  }

  if ('result' in execution && execution.result !== undefined) {
    details.push({ label: 'Result', value: JSON.stringify(execution.result) });
  }

  if ('error' in execution && execution.error) {
    details.push({ label: 'Error', value: execution.error.message });
  }

  details.push(
    { label: 'Created', value: execution.createdAt },
    { label: 'Updated', value: execution.updatedAt },
  );

  const labelWidth = details.reduce((width, detail) => Math.max(width, detail.label.length), 0);

  console.log('');
  for (const detail of details) {
    console.log(`${colors.label(detail.label.padEnd(labelWidth))}  ${detail.value}`);
  }
}
