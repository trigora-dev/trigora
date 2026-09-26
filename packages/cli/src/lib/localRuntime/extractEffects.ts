import ts from 'typescript';

function collectEffectAliases(sourceFile: ts.SourceFile): Set<string> {
  const aliases = new Set<string>();

  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement) || !statement.importClause?.namedBindings) {
      continue;
    }

    if (!ts.isNamedImports(statement.importClause.namedBindings)) {
      continue;
    }

    if (!ts.isStringLiteral(statement.moduleSpecifier)) {
      continue;
    }

    if (statement.moduleSpecifier.text !== '@trigora/sdk') {
      continue;
    }

    for (const element of statement.importClause.namedBindings.elements) {
      if (element.propertyName?.text === 'effect' || element.name.text === 'effect') {
        aliases.add(element.name.text);
      }
    }
  }

  return aliases;
}

function compileHandler(source: string): (input?: unknown) => unknown {
  const factory = new Function(`"use strict"; return (${source});`) as () => (
    input?: unknown,
  ) => unknown;
  const handler = factory();

  return (input?: unknown) => {
    const result = handler(input ?? {});
    if (result && typeof result === 'object' && 'then' in result) {
      throw new Error('Effect callbacks must be synchronous in the local preview.');
    }
    return result;
  };
}

export function extractTypeScriptEffects(
  source: string,
  filename = 'program.ts',
): Record<string, (input?: unknown) => unknown> {
  const sourceFile = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true);
  const aliases = collectEffectAliases(sourceFile);
  const effects: Record<string, () => unknown> = {};

  function visit(node: ts.Node): void {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      aliases.has(node.expression.text) &&
      node.arguments.length >= 2 &&
      ts.isStringLiteralLike(node.arguments[0]!)
    ) {
      const key = node.arguments[0].text;
      const callback = node.arguments[1]!;
      effects[key] = compileHandler(callback.getText(sourceFile));
    }

    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return effects;
}

export function extractTypeScriptEffectSources(
  source: string,
  filename = 'program.ts',
): Record<string, string> {
  const sourceFile = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true);
  const aliases = collectEffectAliases(sourceFile);
  const effects: Record<string, string> = {};

  function visit(node: ts.Node): void {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      aliases.has(node.expression.text) &&
      node.arguments.length >= 2 &&
      ts.isStringLiteralLike(node.arguments[0]!)
    ) {
      const key = node.arguments[0].text;
      effects[key] = node.arguments[1]!.getText(sourceFile);
    }

    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return effects;
}

const PYTHON_EFFECT_SCRIPT = `
import ast, json, sys
source = sys.stdin.read()
tree = ast.parse(source, filename=sys.argv[1])
aliases = {}
handlers = {}
for statement in tree.body:
    if isinstance(statement, ast.ImportFrom) and statement.module == "trigora":
        for alias in statement.names:
            aliases[alias.asname or alias.name] = alias.name

class Visitor(ast.NodeVisitor):
    def visit_Call(self, node):
        if isinstance(node.func, ast.Name) and aliases.get(node.func.id) == "effect":
            if (
                len(node.args) >= 2
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                key = node.args[0].value
                callback = node.args[1]
                if not isinstance(callback, ast.Lambda):
                    raise SystemExit(f"effect \`{key}\` must use a lambda callback")
                fn = eval(compile(ast.Expression(callback), "<effect>", "eval"), {"__builtins__": {}})
                handlers[key] = fn()
        self.generic_visit(node)

Visitor().visit(tree)
json.dump(handlers, sys.stdout)
`;

const PYTHON_EFFECT_SOURCE_SCRIPT = `
import ast, json, sys
source = sys.stdin.read()
tree = ast.parse(source, filename=sys.argv[1])
aliases = {}
handlers = {}
for statement in tree.body:
    if isinstance(statement, ast.ImportFrom) and statement.module == "trigora":
        for alias in statement.names:
            aliases[alias.asname or alias.name] = alias.name

class Visitor(ast.NodeVisitor):
    def visit_Call(self, node):
        if isinstance(node.func, ast.Name) and aliases.get(node.func.id) == "effect":
            if (
                len(node.args) >= 2
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                key = node.args[0].value
                callback = node.args[1]
                handlers[key] = ast.unparse(callback)
        self.generic_visit(node)

Visitor().visit(tree)
json.dump(handlers, sys.stdout)
`;

export { PYTHON_EFFECT_SCRIPT, PYTHON_EFFECT_SOURCE_SCRIPT };
