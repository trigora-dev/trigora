import readline from 'node:readline';

import { compilerVersion, compileProgram, runHandler } from './ts-adapter.js';

const handlers = new Map();

function reply(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function handle(message) {
  if (message.op === 'shutdown') {
    reply({ id: message.id, ok: true });
    process.exit(0);
  }
  if (message.op === 'version') {
    reply({ id: message.id, ok: true, compilerVersion });
    return;
  }
  if (message.op === 'compile') {
    let compiled;
    try {
      compiled = compileProgram(message.source, message.filename, message.programId);
    } catch (error) {
      reply({
        id: message.id,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      });
      return;
    }
    for (const effect of compiled.effects) {
      const handler = compiled.handlers[effect.key];
      if (handler) {
        handlers.set(effect.id, handler);
      }
    }
    reply({
      id: message.id,
      ok: true,
      artifactJson: compiled.artifactJson,
      artifactHash: compiled.artifactHash,
      compilerVersion: compiled.compilerVersion,
      language: compiled.language,
      frontendId: compiled.frontendId,
      frontendVersion: compiled.frontendVersion,
      languageSemanticsVersion: compiled.languageSemanticsVersion,
      engineFormatVersion: compiled.engineFormatVersion,
      effects: compiled.effects,
    });
    return;
  }
  if (message.op === 'effect') {
    const handler = handlers.get(message.handlerId);
    if (!handler) {
      reply({
        id: message.id,
        ok: false,
        error: `No effect handler for \`${message.handlerId}\`.`,
      });
      return;
    }
    try {
      reply({ id: message.id, ok: true, value: runHandler(handler, message.input) });
    } catch (error) {
      reply({
        id: message.id,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      });
    }
    return;
  }
  reply({ id: message.id, ok: false, error: `Unknown helper op \`${message.op}\`.` });
}

const lines = readline.createInterface({ input: process.stdin });
lines.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) {
    return;
  }
  try {
    handle(JSON.parse(trimmed));
  } catch (error) {
    reply({
      id: null,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
});
lines.on('close', () => {
  process.exit(0);
});
