import { extractPythonEffectSources } from './compiler';
import { extractTypeScriptEffectSources } from './extractEffects';
import type { DiscoveredProgram } from './discoverPrograms';
import { extractRustEffects, rustEffectWasm } from './rustEffects';

export type EffectBundleFile = {
  path: string;
  contents: string;
  entrypoint?: boolean;
  encoding?: 'base64';
};

export type EffectBundle = {
  language: 'typescript' | 'python' | 'rust';
  files: EffectBundleFile[];
};

function javascriptWorkerSource(handlers: Record<string, string>): string {
  const entries = Object.entries(handlers)
    .map(([key, source]) => `  ${JSON.stringify(key)}: ${source}`)
    .join(',\n');

  return `const handlers = {
${entries}
};

export default {
  async fetch(request) {
    const body = await request.json();
    const handler = handlers[body.key];
    if (!handler) {
      return Response.json({ error: "Unknown effect " + String(body.key) }, { status: 400 });
    }
    try {
      const result = await handler(body.input);
      return Response.json({ result });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return Response.json({ error: message }, { status: 500 });
    }
  },
};
`;
}

function pythonWorkerSource(handlers: Record<string, string>): string {
  const entries = Object.entries(handlers)
    .map(([key, source]) => `    ${JSON.stringify(key)}: ${source},`)
    .join('\n');

  return `import json
from js import Response

handlers = {
${entries}
}

async def on_fetch(request):
    body = await request.json()
    key = body.get("key")
    handler = handlers.get(key)
    if handler is None:
        return Response.new(
            json.dumps({"error": "Unknown effect " + str(key)}),
            {"status": 400, "headers": {"content-type": "application/json"}},
        )
    try:
        result = handler()
        return Response.new(
            json.dumps({"result": result}),
            {"headers": {"content-type": "application/json"}},
        )
    except Exception as error:
        return Response.new(
            json.dumps({"error": str(error)}),
            {"status": 500, "headers": {"content-type": "application/json"}},
        )
`;
}

function effectManifest(keys: string[]): EffectBundleFile {
  return {
    path: 'effects.json',
    contents: JSON.stringify({ keys }),
  };
}

const RUST_WORKER_SOURCE = `import wasmModule from "./effects.wasm";

let ready;

function load() {
  if (!ready) {
    ready = WebAssembly.instantiate(wasmModule);
  }
  return ready;
}

export default {
  async fetch(request) {
    let body;
    try {
      body = await request.json();
    } catch {
      return Response.json({ error: "effect request requires key and input" }, { status: 400 });
    }
    if (typeof body?.key !== "string" || !Object.hasOwn(body, "input")) {
      return Response.json({ error: "effect request requires key and input" }, { status: 400 });
    }
    const instance = await load();
    const { alloc, handle, out_len: outLen, memory } = instance.exports;
    const payload = new TextEncoder().encode(JSON.stringify({ key: body.key, input: body.input }));
    const ptr = alloc(payload.byteLength);
    new Uint8Array(memory.buffer, ptr, payload.byteLength).set(payload);
    const outPtr = handle(ptr, payload.byteLength);
    const response = JSON.parse(new TextDecoder().decode(new Uint8Array(memory.buffer, outPtr, outLen())));
    return Response.json(response, { status: response.error ? 400 : 200 });
  },
};
`;

export async function buildEffectBundle(
  program: DiscoveredProgram,
  rootDir = process.cwd(),
): Promise<EffectBundle> {
  if (program.language === 'rust') {
    const effects = extractRustEffects(program.source);
    if (effects.length === 0) {
      return { language: 'rust', files: [] };
    }
    const wasm = rustEffectWasm({
      rootDir,
      programId: program.id,
      file: program.file,
      source: program.source,
    });
    return {
      language: 'rust',
      files: [
        { path: 'worker.js', contents: RUST_WORKER_SOURCE, entrypoint: true },
        { path: 'effects.wasm', contents: wasm.toString('base64'), encoding: 'base64' },
        effectManifest(effects.map((effect) => effect.key)),
      ],
    };
  }

  if (program.language === 'python') {
    const handlers = await extractPythonEffectSources(program.source, program.file);
    const keys = Object.keys(handlers);
    if (keys.length === 0) {
      return { language: 'python', files: [] };
    }
    return {
      language: 'python',
      files: [
        {
          path: 'worker.py',
          contents: pythonWorkerSource(handlers),
          entrypoint: true,
        },
        effectManifest(keys),
      ],
    };
  }

  const handlers = extractTypeScriptEffectSources(program.source, program.file);
  const keys = Object.keys(handlers);
  if (keys.length === 0) {
    return { language: 'typescript', files: [] };
  }

  return {
    language: 'typescript',
    files: [
      {
        path: 'worker.js',
        contents: javascriptWorkerSource(handlers),
        entrypoint: true,
      },
      effectManifest(keys),
    ],
  };
}
