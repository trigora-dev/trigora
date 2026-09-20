import { extractPythonEffectSources } from './compiler';
import { extractTypeScriptEffectSources } from './extractEffects';
import type { DiscoveredProgram } from './discoverPrograms';

export type EffectBundleFile = {
  path: string;
  contents: string;
  entrypoint?: boolean;
};

export type EffectBundle = {
  language: 'javascript' | 'python';
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

export async function buildEffectBundle(program: DiscoveredProgram): Promise<EffectBundle> {
  if (program.language === 'python') {
    const handlers = await extractPythonEffectSources(program.source, program.file);
    return {
      language: 'python',
      files: [
        {
          path: 'worker.py',
          contents: pythonWorkerSource(handlers),
          entrypoint: true,
        },
      ],
    };
  }

  return {
    language: 'javascript',
    files: [
      {
        path: 'worker.js',
        contents: javascriptWorkerSource(extractTypeScriptEffectSources(program.source, program.file)),
        entrypoint: true,
      },
    ],
  };
}
