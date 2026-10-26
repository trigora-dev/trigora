import { createHash } from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

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

function compileHandler(source: string, allowPromise: boolean): (input?: unknown) => unknown {
  const factory = new Function(`"use strict"; return (${source});`) as () => (
    input?: unknown,
  ) => unknown;
  const handler = factory();

  return (input?: unknown) => {
    const result = handler(input ?? {});
    if (!allowPromise && result && typeof result === 'object' && 'then' in result) {
      throw new Error('Effect callbacks must be synchronous in the local preview.');
    }
    return result;
  };
}

export function extractTypeScriptEffects(
  source: string,
  filename = 'program.ts',
  options?: { allowPromise?: boolean },
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
      effects[key] = compileHandler(callback.getText(sourceFile), options?.allowPromise === true);
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

const GLOBAL_BINDINGS = new Set([
  'AbortController',
  'Array',
  'BigInt',
  'Boolean',
  'Buffer',
  'Date',
  'Error',
  'Function',
  'Headers',
  'Infinity',
  'Intl',
  'JSON',
  'Map',
  'Math',
  'NaN',
  'Number',
  'Object',
  'Promise',
  'RegExp',
  'Request',
  'Response',
  'Set',
  'String',
  'Symbol',
  'TextDecoder',
  'TextEncoder',
  'URL',
  'URLSearchParams',
  'WeakMap',
  'WeakSet',
  'atob',
  'btoa',
  'clearImmediate',
  'clearInterval',
  'clearTimeout',
  'console',
  'crypto',
  'decodeURI',
  'decodeURIComponent',
  'encodeURI',
  'encodeURIComponent',
  'eval',
  'fetch',
  'globalThis',
  'isFinite',
  'isNaN',
  'parseFloat',
  'parseInt',
  'performance',
  'process',
  'queueMicrotask',
  'setImmediate',
  'setInterval',
  'setTimeout',
  'structuredClone',
  'undefined',
]);

export type HandlerModule = {
  source: string;
  keys: string[];
};

/**
 * A module Node can load from the project. Callbacks keep the program's imports.
 * A name that is neither an import nor a parameter fails before anything claims.
 */
/** Source the TCC frontend can compile. Handler modules keep the original callbacks. */
export function pulseCompileSource(source: string, filename: string): string {
  const sourceFile = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true);
  const aliases = collectEffectAliases(sourceFile);
  const edits: Array<{ start: number; end: number; text: string }> = [];
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement) || !ts.isStringLiteral(statement.moduleSpecifier)) {
      continue;
    }
    if (statement.moduleSpecifier.text !== '@trigora/sdk') {
      edits.push({
        start: statement.getStart(sourceFile),
        end: statement.getEnd(),
        text: '',
      });
    }
  }
  function visit(node: ts.Node): void {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      aliases.has(node.expression.text) &&
      node.arguments.length >= 2 &&
      ts.isStringLiteralLike(node.arguments[0]!)
    ) {
      const callback = node.arguments[1]!;
      const stamp = Number.parseInt(
        createHash('sha256').update(callback.getText(sourceFile)).digest('hex').slice(0, 12),
        16,
      );
      edits.push({
        start: callback.getStart(sourceFile),
        end: callback.getEnd(),
        text: `() => ${stamp}`,
      });
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  edits.sort((left, right) => right.start - left.start);
  let next = source;
  for (const edit of edits) {
    next = next.slice(0, edit.start) + edit.text + next.slice(edit.end);
  }
  return next;
}

export function buildHandlerModule(
  source: string,
  filename: string,
  programFile: string,
): HandlerModule {
  const sourceFile = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true);
  const aliases = collectEffectAliases(sourceFile);
  const imports = importBindings(sourceFile);
  const lines: string[] = [];
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement)) {
      continue;
    }
    const line = emitImport(statement, programFile);
    if (line) {
      lines.push(line);
    }
  }
  const handlers: string[] = [];
  const keys: string[] = [];
  const seen = new Set<string>();

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
      const local = localBinding(callback, sourceFile, imports);
      if (local) {
        throw new Error(`Effect \`${key}\` names local binding \`${local}\`.`);
      }
      if (!seen.has(key)) {
        seen.add(key);
        keys.push(key);
      }
      handlers.push(`  ${JSON.stringify(key)}: ${callback.getText(sourceFile)}`);
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  lines.push('export const handlers = {');
  lines.push(handlers.join(',\n'));
  lines.push('};');
  lines.push('');
  return { source: lines.join('\n'), keys };
}

function importBindings(sourceFile: ts.SourceFile): Set<string> {
  const names = new Set<string>();
  for (const statement of sourceFile.statements) {
    if (!ts.isImportDeclaration(statement) || statement.importClause?.isTypeOnly) {
      continue;
    }
    const clause = statement.importClause;
    if (!clause) {
      continue;
    }
    if (clause.name) {
      names.add(clause.name.text);
    }
    const named = clause.namedBindings;
    if (!named) {
      continue;
    }
    if (ts.isNamespaceImport(named)) {
      names.add(named.name.text);
      continue;
    }
    for (const element of named.elements) {
      if (!element.isTypeOnly) {
        names.add(element.name.text);
      }
    }
  }
  return names;
}

function emitImport(statement: ts.ImportDeclaration, programFile: string): string | undefined {
  if (!ts.isStringLiteral(statement.moduleSpecifier) || statement.importClause?.isTypeOnly) {
    return undefined;
  }
  const spec = JSON.stringify(runtimeSpecifier(statement.moduleSpecifier.text, programFile));
  const clause = statement.importClause;
  if (!clause) {
    return `import ${spec};`;
  }
  const defaultName = clause.name?.text;
  const named = clause.namedBindings;
  let namespace = '';
  const elements: string[] = [];
  if (named && ts.isNamespaceImport(named)) {
    namespace = named.name.text;
  }
  if (named && ts.isNamedImports(named)) {
    for (const element of named.elements) {
      if (element.isTypeOnly) {
        continue;
      }
      elements.push(
        element.propertyName
          ? `${element.propertyName.text} as ${element.name.text}`
          : element.name.text,
      );
    }
  }
  if (!defaultName && !namespace && elements.length === 0) {
    return undefined;
  }
  const head: string[] = [];
  if (defaultName) {
    head.push(defaultName);
  }
  if (namespace) {
    head.push(`* as ${namespace}`);
  }
  if (elements.length > 0) {
    head.push(`{ ${elements.join(', ')} }`);
  }
  return `import ${head.join(', ')} from ${spec};`;
}

function runtimeSpecifier(specifier: string, programFile: string): string {
  if (specifier.startsWith('.')) {
    return pathToFileURL(path.resolve(path.dirname(programFile), specifier)).href;
  }
  return specifier;
}

function localBinding(
  callback: ts.Node,
  sourceFile: ts.SourceFile,
  imports: Set<string>,
): string | undefined {
  const declared = new Set<string>();
  collectDeclared(callback, declared);
  let found: string | undefined;
  collectReferenced(callback, (name) => {
    if (found || declared.has(name) || imports.has(name) || GLOBAL_BINDINGS.has(name)) {
      return;
    }
    found = name;
  });
  return found;
}

function collectDeclared(node: ts.Node, declared: Set<string>): void {
  if (ts.isTypeNode(node)) {
    return;
  }
  if (
    ts.isFunctionDeclaration(node) ||
    ts.isFunctionExpression(node) ||
    ts.isArrowFunction(node) ||
    ts.isMethodDeclaration(node) ||
    ts.isConstructorDeclaration(node)
  ) {
    for (const parameter of node.parameters) {
      bindingNames(parameter.name, declared);
    }
    if (ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node)) {
      if (node.name) {
        declared.add(node.name.text);
      }
    }
  }
  if (ts.isVariableDeclaration(node) || ts.isBindingElement(node)) {
    bindingNames(node.name, declared);
  }
  if (ts.isClassDeclaration(node) || ts.isClassExpression(node)) {
    if (node.name) {
      declared.add(node.name.text);
    }
  }
  if (ts.isCatchClause(node) && node.variableDeclaration) {
    bindingNames(node.variableDeclaration.name, declared);
  }
  ts.forEachChild(node, (child) => collectDeclared(child, declared));
}

function bindingNames(name: ts.BindingName, declared: Set<string>): void {
  if (ts.isIdentifier(name)) {
    declared.add(name.text);
    return;
  }
  for (const element of name.elements) {
    if (!ts.isOmittedExpression(element)) {
      bindingNames(element.name, declared);
    }
  }
}

function collectReferenced(node: ts.Node, visitName: (name: string) => void): void {
  if (ts.isTypeNode(node)) {
    return;
  }
  if (ts.isIdentifier(node) && !isDeclarationName(node) && !isPropertyName(node)) {
    visitName(node.text);
  }
  ts.forEachChild(node, (child) => collectReferenced(child, visitName));
}

function isDeclarationName(node: ts.Identifier): boolean {
  const parent = node.parent;
  if (ts.isParameter(parent) || ts.isVariableDeclaration(parent) || ts.isBindingElement(parent)) {
    return parent.name === node;
  }
  if (
    (ts.isFunctionDeclaration(parent) ||
      ts.isFunctionExpression(parent) ||
      ts.isClassDeclaration(parent) ||
      ts.isClassExpression(parent)) &&
    parent.name === node
  ) {
    return true;
  }
  return false;
}

function isPropertyName(node: ts.Identifier): boolean {
  const parent = node.parent;
  if (ts.isPropertyAccessExpression(parent)) {
    return parent.name === node;
  }
  if (ts.isPropertyAssignment(parent) || ts.isMethodDeclaration(parent)) {
    return parent.name === node;
  }
  return false;
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
