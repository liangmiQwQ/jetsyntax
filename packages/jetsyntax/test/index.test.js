import { describe, expect, it } from "vitest";

import { parseToTape } from "../binding.js";
import { parse } from "../index.js";

describe("parse", () => {
  it("returns materialized ESTree for TSX", () => {
    const result = parse("const view: JSX.Element = <output>{42n}</output>", { lang: "tsx" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.type).toBe("Program");
    expect(result.program.body).toHaveLength(1);
  });

  it("returns Babel-compatible TypeScript import-equals nodes", () => {
    const source = [
      "import Alias = Namespace.Deep;",
      "import external = require(\"package\");",
      "import type types = require(\"types\");",
      "export import Public = Namespace.Member;",
      "export import type PublicTypes = require(\"public-types\");",
    ].join("\n");
    const result = parse(source, { lang: "ts", semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "TSImportEqualsDeclaration",
        importKind: "value",
        id: { type: "Identifier", name: "Alias" },
        moduleReference: {
          type: "TSQualifiedName",
          left: { type: "Identifier", name: "Namespace" },
          right: { type: "Identifier", name: "Deep" },
        },
      },
      {
        type: "TSImportEqualsDeclaration",
        importKind: "value",
        id: { type: "Identifier", name: "external" },
        moduleReference: {
          type: "TSExternalModuleReference",
          expression: { type: "Literal", value: "package", raw: "\"package\"" },
        },
      },
      {
        type: "TSImportEqualsDeclaration",
        importKind: "type",
        id: { type: "Identifier", name: "types" },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "value",
        source: null,
        specifiers: [],
        attributes: [],
        declaration: {
          type: "TSImportEqualsDeclaration",
          importKind: "value",
          id: { type: "Identifier", name: "Public" },
        },
      },
      {
        type: "ExportNamedDeclaration",
        declaration: {
          type: "TSImportEqualsDeclaration",
          importKind: "type",
          id: { type: "Identifier", name: "PublicTypes" },
          moduleReference: {
            type: "TSExternalModuleReference",
            expression: { type: "Literal", value: "public-types" },
          },
        },
      },
    ]);
  });

  it("isolates lexical bindings across every for-loop form", () => {
    const source = [
      "let index = 0;",
      "for (let index = 0; index < 1; index++) { for (let index = 0; index < 1; index++) {} }",
      "for (let index = 0; index < 1; index++) {}",
      "const key = 'outer';",
      "for (const [key] in first) {}",
      "for (const [key] in second) {}",
      "const value = 'outer';",
      "for (const { value } of first) {}",
      "for (const { value } of second) {}",
      "for (let blockValue = 0; false;) { function blockValue() {} }",
      "async function consume() {",
      "  for await (const value of first) {}",
      "  for await (const value of second) {}",
      "}",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const topLevelLoops = result.program.body.filter(node => node.type.startsWith("For"));
    expect(topLevelLoops.map(node => node.type)).toEqual([
      "ForStatement",
      "ForStatement",
      "ForInStatement",
      "ForInStatement",
      "ForOfStatement",
      "ForOfStatement",
      "ForStatement",
    ]);
    expect(topLevelLoops[2].left.declarations[0].id.type).toBe("ArrayPattern");
    expect(topLevelLoops[4].left.declarations[0].id.type).toBe("ObjectPattern");
    const asynchronousLoops = result.program.body.at(-1).body.body;
    expect(asynchronousLoops).toMatchObject([
      { type: "ForOfStatement", await: true },
      { type: "ForOfStatement", await: true },
    ]);
    expect(
      parse("'use strict'; for (let value = 0; false;) { function value() {} }").diagnostics,
    ).toEqual([]);
    expect(
      parse("{ function value() {} function value() {} }", { sourceType: "script" }).diagnostics,
    ).toEqual([]);
  });

  it("retains lexical for-head conflicts and restores the scope after recovery", () => {
    for (
      const source of [
        "for (let value = 0, value = 1; false;) {}",
        "for (let value = 0; false;) { var value; }",
        "{ let value; function value() {} }",
        "'use strict'; { function value() {} function value() {} }",
        "try {} catch (value) { function value() {} }",
      ]
    ) {
      const result = parse(source, { semanticErrors: true });

      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    const recovered = parse("for (let leaked = 0; false;) { const = 1; } let leaked;", {
      semanticErrors: true,
    });
    expect(recovered.diagnostics).not.toEqual([]);
    expect(recovered.diagnostics).not.toContain("duplicate binding `leaked`");
  });

  it("diagnoses invalid regular-expression literal flags with a recovered ESTree node", () => {
    for (const [source, flags] of [["/./G;", "G"], ["/./gig;", "gig"], ["/./uv;", "uv"]]) {
      const result = parse(source, { semanticErrors: true });

      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
      expect(result.program.body[0].expression).toMatchObject({
        type: "Literal",
        raw: source.slice(0, -1),
        value: null,
        regex: { pattern: ".", flags },
      });
    }
  });

  it("leaves regular-expression constructor validation to runtime", () => {
    const result = parse("new RegExp(\".\", \"uv\"); RegExp(\"\\\\p{Unknown}\", \"u\");");

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      { expression: { type: "NewExpression" } },
      { expression: { type: "CallExpression" } },
    ]);
  });

  it("diagnoses direct import call new callees while preserving recovered ESTree", () => {
    const recovered = parse("new import(\"package\").then;");

    expect(recovered.diagnostics).not.toEqual([]);
    expect(recovered.program.body[0].expression).toMatchObject({
      type: "NewExpression",
      callee: {
        type: "MemberExpression",
        object: {
          type: "ImportExpression",
          source: { type: "Literal", value: "package" },
        },
      },
    });

    for (const source of ["new (import('package'));", "new import.meta();"]) {
      expect(parse(source, { sourceType: "module" }).diagnostics, source).toEqual([]);
    }
  });

  it("keeps statement-leading dynamic imports on the expression postfix path", () => {
    const result = parse(
      [
        "import('bare');",
        "import('chain').then(handler).catch(handler);",
        "import('call')();",
        "import('tag')``;",
        "import",
        "('line-break').then(handler);",
      ].join("\n"),
      { sourceType: "script" },
    );

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      { expression: { type: "ImportExpression" } },
      { expression: { type: "CallExpression" } },
      { expression: { type: "CallExpression", callee: { type: "ImportExpression" } } },
      { expression: { type: "TaggedTemplateExpression", tag: { type: "ImportExpression" } } },
      { expression: { type: "CallExpression" } },
    ]);

    const staticImport = parse("import value from 'package';", { sourceType: "module" });
    expect(staticImport.diagnostics).toEqual([]);
    expect(staticImport.program.body).toMatchObject([{ type: "ImportDeclaration" }]);

    const malformed = parse("import();", { sourceType: "script" });
    expect(malformed.diagnostics).not.toEqual([]);
    expect(malformed.program.type).toBe("Program");
  });

  it("decodes import-dot primary expressions and their phase", () => {
    const result = parse(
      [
        "import.meta;",
        "import.source('source').then(use);",
        "import.defer('defer', { with: { type: 'json' } })();",
      ].join("\n"),
      { sourceType: "module" },
    );

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        expression: {
          type: "MetaProperty",
          meta: { type: "Identifier", name: "import" },
          property: { type: "Identifier", name: "meta" },
        },
      },
      {
        expression: {
          type: "CallExpression",
          callee: {
            type: "MemberExpression",
            object: { type: "ImportExpression", phase: "source", options: null },
          },
        },
      },
      {
        expression: {
          type: "CallExpression",
          callee: {
            type: "ImportExpression",
            phase: "defer",
            source: { type: "Literal", value: "defer" },
            options: { type: "ObjectExpression" },
          },
        },
      },
    ]);
  });

  it("recovers malformed import-dot forms with focused diagnostics", () => {
    const malformed = parse(
      [
        "import.source();",
        "import.defer('source', {}, extra);",
        "import.source(...arguments);",
        "new import.defer('source').then;",
        "import.source('source') = value;",
        "function f(...import.defer('source')) {}",
      ].join("\n"),
      { semanticErrors: true },
    );
    expect(malformed.diagnostics.length).toBeGreaterThanOrEqual(6);
    expect(malformed.program.type).toBe("Program");

    for (
      const [source, options] of [
        ["import.meta;", { sourceType: "script" }],
        ["import.unknown;", { sourceType: "module" }],
        ["import.m\\u0065ta;", { sourceType: "module" }],
        ["import.sour\\u0063e('source');", { sourceType: "script" }],
      ]
    ) {
      const result = parse(source, options);
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type, source).toBe("Program");
    }
  });

  it("diagnoses escaped reserved identifiers only in reference and binding positions", () => {
    for (
      const source of [
        "br\\u0065ak;",
        "var br\\u{65}ak;",
        "tru\\u0065: statement;",
        "({ br\\u0065ak } = source);",
        "({ br\\u0065ak }) => {};",
        "'use strict'; ({ impl\\u0065ments });",
        "async function f() { ({ aw\\u0061it }); }",
        "function* f() { ({ yi\\u0065ld }); }",
        "class C { static field = { await }; }",
        "function f() { n\\u0065w.target; }",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });

      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    const allowed = parse(
      "const object = { br\\u0065ak: 1 }; object.br\\u0065ak; class C { br\\u0065ak() {} } const { br\\u0065ak: value } = object;",
      { semanticErrors: true, sourceType: "script" },
    );
    expect(allowed.diagnostics).toEqual([]);

    const syntaxOnly = parse("let br\\u0065ak = 1; ({ br\\u0065ak });", {
      lang: "ts",
      semanticErrors: false,
      sourceType: "script",
    });
    expect(syntaxOnly.diagnostics).toEqual([]);
  });

  it("materializes AST output containing braced Unicode identifier escapes", () => {
    const result = parse("<\\u{2F804}></\\u{2F804}>", { lang: "jsx", semanticErrors: true });

    expect(result.program.type).toBe("Program");
    expect(result.diagnostics).toEqual([]);
    const element = result.program.body[0].expression;
    expect(element.openingElement.name.name).toBe("\\u{2F804}");
    expect(element.closingElement.name.name).toBe("\\u{2F804}");
  });

  it("bounds recovery for an unterminated braced Unicode string escape", () => {
    const result = parse("var value = \"\\u{67\";");

    expect(result.program.body[0].declarations[0].init).toMatchObject({
      type: "Literal",
      raw: "\"\\u{67\"",
      value: "u{67",
    });
  });

  it("transfers native output as a Uint32Array", () => {
    const result = parseToTape("answer + 1", { range: true });

    expect(result.tape).toBeInstanceOf(Uint32Array);
    expect(result.tape[0]).toBe(0x4A53_5450);
    expect(result.tape[4]).toBe(result.tape.length);
    expect(result.diagnostics).toEqual([]);
  });

  it("decodes newly emitted statement, module, class, and template schemas", () => {
    const source = [
      "import { value } from \"source\";",
      "export class Example { method() { return `value: ${value}`; } }",
      "switch (value) { case 1: break; default: debugger; }",
      "try {} catch (error) { error; }",
    ].join("\n");
    const result = parse(source);

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body.map(node => node.type)).toEqual([
      "ImportDeclaration",
      "ExportNamedDeclaration",
      "SwitchStatement",
      "TryStatement",
    ]);
    const method = result.program.body[1].declaration.body.body[0];
    expect(method.value.body.body[0].argument.type).toBe("TemplateLiteral");
  });

  it("materializes zero-parameter arrow functions", () => {
    const source = [
      "const expression = () => 1;",
      "const block = (/* parameters */) /* arrow */ => { return 2; };",
      "const nested = promise.then(() => value);",
      "const invoked = (() => value)();",
    ].join("\n");
    const result = parse(source, {
      preserveParens: true,
      semanticErrors: true,
      sourceType: "script",
    });

    expect(result.diagnostics).toEqual([]);
    const [expression, block, nested, invoked] = result.program.body.map(
      statement => statement.declarations[0].init,
    );
    expect(expression).toMatchObject({
      type: "ArrowFunctionExpression",
      id: null,
      params: [],
      body: { type: "Literal", value: 1 },
      generator: false,
      async: false,
      expression: true,
    });
    expect(source.slice(expression.start, expression.end)).toBe("() => 1");
    expect(block).toMatchObject({
      type: "ArrowFunctionExpression",
      params: [],
      body: { type: "BlockStatement" },
      async: false,
      expression: false,
    });
    expect(source.slice(block.start, block.end)).toBe(
      "(/* parameters */) /* arrow */ => { return 2; }",
    );
    expect(nested.arguments[0]).toMatchObject({
      type: "ArrowFunctionExpression",
      params: [],
      body: { type: "Identifier", name: "value" },
    });
    expect(invoked.callee).toMatchObject({
      type: "ParenthesizedExpression",
      expression: { type: "ArrowFunctionExpression", params: [] },
    });

    const unwrapped = parse("const invoked = (() => value)();", {
      preserveParens: false,
      sourceType: "script",
    });
    expect(unwrapped.diagnostics).toEqual([]);
    expect(unwrapped.program.body[0].declarations[0].init.callee).toMatchObject({
      type: "ArrowFunctionExpression",
      params: [],
    });
  });

  it("materializes parenthesized rest-arrow parameters", () => {
    const source = [
      "const direct = (...args) => args;",
      "const prefixed = (first, second, third, ...rest) => rest;",
      "const destructured = (...[first, second]) => first;",
      "const asynchronous = async (...args) => await invoke(...args);",
    ].join("\n");
    const result = parse(source, { semanticErrors: true, sourceType: "script" });

    expect(result.diagnostics).toEqual([]);
    const [direct, prefixed, destructured, asynchronous] = result.program.body.map(
      statement => statement.declarations[0].init,
    );
    expect(direct).toMatchObject({
      type: "ArrowFunctionExpression",
      params: [{ type: "RestElement", argument: { type: "Identifier", name: "args" } }],
    });
    expect(prefixed.params.at(-1)).toMatchObject({
      type: "RestElement",
      argument: { type: "Identifier", name: "rest" },
    });
    expect(destructured.params).toMatchObject([
      { type: "RestElement", argument: { type: "ArrayPattern" } },
    ]);
    expect(asynchronous).toMatchObject({
      type: "ArrowFunctionExpression",
      async: true,
      params: [{ type: "RestElement", argument: { type: "Identifier", name: "args" } }],
      body: { type: "AwaitExpression" },
    });
  });

  it("preserves non-rest parenthesized expression paths", () => {
    const result = parse(
      "const assigned = (value = fallback); const destructured = ({ value } = source);",
      { preserveParens: true, semanticErrors: true, sourceType: "script" },
    );

    expect(result.diagnostics).toEqual([]);
    for (const statement of result.program.body) {
      expect(statement.declarations[0].init.type).toBe("ParenthesizedExpression");
    }
  });

  it("diagnoses invalid rest-arrow parameters", () => {
    for (
      const source of [
        "const invalid = (...args = []) => args;",
        "const invalid = (...args,) => args;",
        "const invalid = (...args, value) => args;",
        "const invalid = (...first, ...second) => first;",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });

      expect(result.diagnostics, source).not.toEqual([]);
      const arrow = result.program.body[0].declarations[0].init;
      expect(arrow.type).toBe("ArrowFunctionExpression");
      expect(arrow.params.some(parameter => parameter.type === "RestElement")).toBe(true);
    }

    const nestedAwait = parse("async(value = (...await) => {}) => {};", {
      semanticErrors: true,
      sourceType: "script",
    });
    expect(nestedAwait.diagnostics).not.toEqual([]);
    expect(nestedAwait.program.body[0].expression.type).toBe("ArrowFunctionExpression");
  });

  it("rejects line terminators before zero-parameter arrow tokens", () => {
    for (const source of ["const callback = ()\n=> value;", "const callback = ()/*\n*/=> value;"]) {
      const result = parse(source, { sourceType: "script" });

      expect(result.diagnostics).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("recovers truncated zero-parameter arrow bodies", () => {
    const result = parse("const callback = () =>", { sourceType: "script" });

    expect(result.diagnostics).not.toEqual([]);
    expect(result.program.body[0].declarations[0].init).toMatchObject({
      type: "ArrowFunctionExpression",
      params: [],
      async: false,
      expression: true,
    });
  });

  it("materializes named and default exported async functions", () => {
    const source = [
      "export async function load() { return await fetchValue(); }",
      "export default async function* stream() { yield await next(); }",
    ].join("\n");
    const result = parse(source);

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "ExportNamedDeclaration",
        declaration: {
          type: "FunctionDeclaration",
          generator: false,
          async: true,
        },
      },
      {
        type: "ExportDefaultDeclaration",
        declaration: {
          type: "FunctionDeclaration",
          generator: true,
          async: true,
        },
      },
    ]);
  });

  it("materializes async function and generator expressions", () => {
    const source = [
      "const anonymous = async function (value) { return await load(value); };",
      "const named = async function* stream() { yield await next(); };",
    ].join("\n");
    const result = parse(source, { sourceType: "script" });

    expect(result.diagnostics).toEqual([]);
    const [anonymous, named] = result.program.body.map(statement => statement.declarations[0].init);
    expect(anonymous).toMatchObject({
      type: "FunctionExpression",
      id: null,
      params: [{ type: "Identifier", name: "value" }],
      generator: false,
      async: true,
    });
    expect(source.slice(anonymous.start, anonymous.end)).toBe(
      "async function (value) { return await load(value); }",
    );
    expect(named).toMatchObject({
      type: "FunctionExpression",
      id: { type: "Identifier", name: "stream" },
      generator: true,
      async: true,
    });
    expect(source.slice(named.start, named.end)).toBe(
      "async function* stream() { yield await next(); }",
    );
  });

  it("keeps escaped and line-broken async function expressions separate", () => {
    const lineBreak = parse("const value = async\nfunction split() {}", { sourceType: "script" });
    expect(lineBreak.diagnostics).toEqual([]);
    expect(lineBreak.program.body).toMatchObject([
      { declarations: [{ init: { type: "Identifier", name: "async" } }] },
      { type: "FunctionDeclaration", async: false, generator: false },
    ]);

    const escaped = parse("const value = \\u0061sync function split() {}", {
      sourceType: "script",
    });
    expect(escaped.diagnostics).not.toEqual([]);
    expect(escaped.program.body[0].declarations[0].init.type).toBe("Identifier");
  });

  it("diagnoses async function expression early errors", () => {
    const allowedDeclaration = parse("async function await() {}", {
      semanticErrors: true,
      sourceType: "script",
    });
    expect(allowedDeclaration.diagnostics).toEqual([]);

    for (
      const source of [
        "const value = async function await() {};",
        "const value = async function* await() {};",
        "const value = async function() { var \\u0061wait; };",
        "const value = async function() { void \\u0061wait; };",
        "const value = async function* yield() {};",
        "const value = async function*() { var \\u0079ield; };",
        "const value = async function*() { void \\u0079ield; };",
        "const value = async function(input = await source) {};",
        "const value = async function*(input = yield source) {};",
        "const value = async function(input = source) { 'use strict'; };",
        "const value = async function(...inputs,) {};",
        "const value = async function(input) { let input; };",
        "const value = async function*() { super.value; };",
        "(async function() {}) = value;",
        "const value = async function*() { yield\n* source; };",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });

      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("inherits super through arrows but resets it for nested functions", () => {
    const inherited = parse(
      "const object = { method() { return () => super.value; } };",
      { semanticErrors: true },
    );
    expect(inherited.diagnostics).toEqual([]);

    const reset = parse(
      "class Child extends Parent { method() { return function() { return super.method(); }; } }",
      { semanticErrors: true },
    );
    expect(reset.diagnostics).not.toEqual([]);
  });

  it("materializes TypeScript function return annotations", () => {
    const source = [
      "function convert(value: Input): Namespace.Output { return value; }",
      "const later = function* (): Iterable<Result> { yield result; };",
      "async function load(): Promise<Result> { return await request(); }",
      "function plain() {}",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    const [convert, expressionStatement, load, plain] = result.program.body;
    expect(convert).toMatchObject({
      type: "FunctionDeclaration",
      returnType: {
        type: "TSTypeAnnotation",
        typeAnnotation: {
          type: "TSTypeReference",
          typeName: {
            type: "TSQualifiedName",
            left: { name: "Namespace" },
            right: { name: "Output" },
          },
        },
      },
    });
    expect(source.slice(convert.returnType.start, convert.returnType.end)).toBe(
      ": Namespace.Output",
    );
    expect(expressionStatement.declarations[0].init).toMatchObject({
      type: "FunctionExpression",
      generator: true,
      returnType: {
        type: "TSTypeAnnotation",
        typeAnnotation: { type: "TSTypeReference" },
      },
    });
    expect(load).toMatchObject({
      type: "FunctionDeclaration",
      async: true,
      returnType: { type: "TSTypeAnnotation" },
    });
    expect(plain).not.toHaveProperty("returnType");
  });

  it("materializes TypeScript property signatures without annotations", () => {
    const source = [
      "interface Shape {",
      "  plain;",
      "  optional?,",
      "  readonly inferred",
      "  \"quoted\";",
      "  0?",
      "  typed: string",
      "}",
      "type Literal = { left; right?: number }",
      "export {};",
      "declare global { interface Array<T> { x } }",
    ].join("\n");
    const result = parse(source, { lang: "ts", sourceType: "module", range: true });

    expect(result.diagnostics).toEqual([]);
    const interfaceProperties = result.program.body[0].body.body;
    const literalProperties = result.program.body[1].typeAnnotation.members;
    const globalProperty = result.program.body[3].body.body[0].body.body[0];
    const properties = [...interfaceProperties, ...literalProperties, globalProperty];
    expect(properties).toMatchObject([
      { type: "TSPropertySignature", key: { name: "plain" }, typeAnnotation: null },
      {
        type: "TSPropertySignature",
        key: { name: "optional" },
        typeAnnotation: null,
        optional: true,
      },
      {
        type: "TSPropertySignature",
        key: { name: "inferred" },
        typeAnnotation: null,
        readonly: true,
      },
      {
        type: "TSPropertySignature",
        key: { type: "Literal", value: "quoted" },
        typeAnnotation: null,
      },
      {
        type: "TSPropertySignature",
        key: { type: "Literal", value: 0 },
        typeAnnotation: null,
        optional: true,
      },
      {
        type: "TSPropertySignature",
        key: { name: "typed" },
        typeAnnotation: { typeAnnotation: { type: "TSStringKeyword" } },
      },
      { type: "TSPropertySignature", key: { name: "left" }, typeAnnotation: null },
      {
        type: "TSPropertySignature",
        key: { name: "right" },
        typeAnnotation: { typeAnnotation: { type: "TSNumberKeyword" } },
        optional: true,
      },
      { type: "TSPropertySignature", key: { name: "x" }, typeAnnotation: null },
    ]);
    expect(properties.map(property => source.slice(property.start, property.end))).toEqual([
      "plain",
      "optional?",
      "readonly inferred",
      "\"quoted\"",
      "0?",
      "typed: string",
      "left",
      "right?: number",
      "x",
    ]);
    for (const property of properties) {
      expect(property.computed).toBe(false);
      expect(property.range).toEqual([property.start, property.end]);
    }
  });

  it("preserves readonly as a type-member name when it has no member follower", () => {
    const source = [
      "interface Names {",
      "  readonly;",
      "  readonly: boolean;",
      "  readonly?;",
      "  readonly(): void;",
      "  readonly",
      "  following",
      "  readonly value",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].body.body).toMatchObject([
      { type: "TSPropertySignature", key: { name: "readonly" }, readonly: false },
      {
        type: "TSPropertySignature",
        key: { name: "readonly" },
        typeAnnotation: { typeAnnotation: { type: "TSBooleanKeyword" } },
        readonly: false,
      },
      {
        type: "TSPropertySignature",
        key: { name: "readonly" },
        optional: true,
        readonly: false,
      },
      { type: "TSMethodSignature", key: { name: "readonly" } },
      { type: "TSPropertySignature", key: { name: "readonly" }, readonly: false },
      { type: "TSPropertySignature", key: { name: "following" }, readonly: false },
      { type: "TSPropertySignature", key: { name: "value" }, readonly: true },
    ]);
  });

  it("keeps unsupported and JavaScript type-member forms diagnostic", () => {
    const sameLine = parse("interface Broken { first second }", { lang: "ts" });
    expect(sameLine.diagnostics.some(diagnostic => diagnostic.includes("type member separator")))
      .toBe(true);
    expect(sameLine.program.body[0].body.body).toMatchObject([
      { type: "TSPropertySignature", key: { name: "first" }, typeAnnotation: null },
      { type: "TSPropertySignature", key: { name: "second" }, typeAnnotation: null },
    ]);

    for (
      const source of [
        "interface I { field = 1; }",
        "interface I { [computed]?; }",
        "interface I { [key: string]: number; }",
        "interface I { (): void; }",
        "interface I { get value(): string; }",
      ]
    ) {
      expect(parse(source, { lang: "ts" }).diagnostics, source).not.toEqual([]);
    }
    for (const lang of ["js", "jsx"]) {
      const result = parse("interface Shape { value }", { lang });
      expect(result.diagnostics, lang).not.toEqual([]);
      expect(JSON.stringify(result.program), lang).not.toContain("TSPropertySignature");
    }
  });

  it("materializes TypeScript runtime function type parameters", () => {
    const source = [
      "function convert<T extends Input, U = T>(value: T): U { return value; }",
      "const later = function<const T>(value: T) { return value; };",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    const declaration = result.program.body[0];
    const expression = result.program.body[1].declarations[0].init;
    expect(declaration).toMatchObject({
      type: "FunctionDeclaration",
      returnType: { type: "TSTypeAnnotation" },
      typeParameters: {
        type: "TSTypeParameterDeclaration",
        params: [
          {
            type: "TSTypeParameter",
            name: { type: "Identifier", name: "T" },
            constraint: { type: "TSTypeReference" },
            default: null,
          },
          {
            type: "TSTypeParameter",
            name: { type: "Identifier", name: "U" },
            constraint: null,
            default: { type: "TSTypeReference" },
          },
        ],
      },
    });
    expect(expression).toMatchObject({
      type: "FunctionExpression",
      typeParameters: {
        type: "TSTypeParameterDeclaration",
        params: [{ type: "TSTypeParameter", const: true }],
      },
    });
    expect(expression).not.toHaveProperty("returnType");
    expect(source.slice(declaration.typeParameters.start, declaration.typeParameters.end)).toBe(
      "<T extends Input, U = T>",
    );
    expect(source.slice(expression.typeParameters.start, expression.typeParameters.end)).toBe("<const T>");
  });

  it("keeps runtime function type parameters out of JavaScript", () => {
    for (const lang of ["js", "jsx"]) {
      const result = parse("function invalid<T>(value) {}", { lang });

      expect(result.diagnostics, lang).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("diagnoses empty runtime function type parameters", () => {
    const empty = parse("function invalid<>() {}", { lang: "ts" });

    expect(empty.diagnostics).not.toEqual([]);
    expect(empty.program.body[0].typeParameters).toMatchObject({
      type: "TSTypeParameterDeclaration",
      params: [],
    });
  });

  it("materializes direct TypeScript generic new expressions", () => {
    const source = [
      "new Plain();",
      "new Factory<Input>(value);",
      "new Namespace.Factory<Map<Key, Value>>;",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [plain, generic, nested] = result.program.body.map(statement => statement.expression);
    expect(plain).toMatchObject({
      type: "NewExpression",
      callee: { type: "Identifier", name: "Plain" },
      arguments: [],
    });
    expect(plain).not.toHaveProperty("typeArguments");
    expect(generic).toMatchObject({
      type: "NewExpression",
      callee: { name: "Factory" },
      arguments: [{ name: "value" }],
      typeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ type: "TSTypeReference", typeName: { name: "Input" } }],
      },
    });
    expect(source.slice(generic.start, generic.end)).toBe("new Factory<Input>(value)");
    expect(source.slice(generic.typeArguments.start, generic.typeArguments.end)).toBe("<Input>");
    expect(nested).toMatchObject({
      type: "NewExpression",
      callee: {
        type: "MemberExpression",
        object: { name: "Namespace" },
        property: { name: "Factory" },
      },
      typeArguments: {
        params: [{ typeName: { name: "Map" }, typeArguments: { params: [{}, {}] } }],
      },
    });

    const empty = parse("new Factory<>();", { lang: "ts" });
    expect(empty.diagnostics).not.toEqual([]);
    expect(empty.program.body[0].expression.typeArguments).toMatchObject({
      type: "TSTypeParameterInstantiation",
      params: [],
    });

    const tsx = parse("new Factory<Input>();", { lang: "tsx" });
    expect(tsx.diagnostics).toEqual([]);
    expect(tsx.program.body[0].expression).toHaveProperty("typeArguments");

    const relational = parse("new Factory<Input>=value;", { lang: "ts" });
    expect(relational.diagnostics).toEqual([]);
    expect(relational.program.body[0].expression).toMatchObject({
      type: "BinaryExpression",
      operator: ">=",
      left: {
        type: "BinaryExpression",
        operator: "<",
        left: { type: "NewExpression", callee: { name: "Factory" } },
        right: { name: "Input" },
      },
      right: { name: "value" },
    });
    expect(relational.program.body[0].expression.left.left).not.toHaveProperty("typeArguments");

    const shiftAssign = parse("new Factory<Input>>=value;", { lang: "ts" });
    expect(shiftAssign.diagnostics).not.toEqual([]);
    expect(shiftAssign.program.body[0].expression).toMatchObject({
      type: "AssignmentExpression",
      operator: ">>=",
      left: {
        type: "BinaryExpression",
        operator: "<",
        left: { type: "NewExpression", callee: { name: "Factory" } },
      },
      right: { name: "value" },
    });
    expect(shiftAssign.program.body[0].expression.left.left).not.toHaveProperty("typeArguments");
  });

  it("materializes TypeScript class implements clauses without widening plain classes", () => {
    const source = [
      "class Plain {}",
      "class Derived extends Base implements First, Namespace.Second<Map<Key, Value>> {}",
      "const Anonymous = class implements Callable<Argument> {};",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [plain, derived, anonymousDeclaration] = result.program.body;
    expect(plain).toMatchObject({
      type: "ClassDeclaration",
      id: { name: "Plain" },
      superClass: null,
    });
    expect(plain).not.toHaveProperty("implements");
    expect(derived).toMatchObject({
      type: "ClassDeclaration",
      superClass: { name: "Base" },
      implements: [
        {
          type: "TSClassImplements",
          expression: { name: "First" },
          typeArguments: null,
        },
        {
          type: "TSClassImplements",
          expression: {
            type: "MemberExpression",
            object: { name: "Namespace" },
            property: { name: "Second" },
            computed: false,
          },
          typeArguments: {
            type: "TSTypeParameterInstantiation",
            params: [{
              typeName: { name: "Map" },
              typeArguments: { params: [{}, {}] },
            }],
          },
        },
      ],
    });
    expect(source.slice(derived.implements[1].start, derived.implements[1].end)).toBe(
      "Namespace.Second<Map<Key, Value>>",
    );
    expect(derived.implements[1].range).toEqual([
      derived.implements[1].start,
      derived.implements[1].end,
    ]);
    expect(anonymousDeclaration.declarations[0].init).toMatchObject({
      type: "ClassExpression",
      id: null,
      implements: [{ expression: { name: "Callable" } }],
    });

    for (const semanticErrors of [false, true]) {
      const parsed = parse("class Empty implements {} class Generic implements Box<> {}", {
        lang: "ts",
        semanticErrors,
      });
      expect(parsed.diagnostics.length === 0).toBe(!semanticErrors);
      expect(parsed.program.body).toMatchObject([
        { implements: [] },
        { implements: [{ typeArguments: { params: [] } }] },
      ]);
    }

    const compatibility = parse("class Compatible implements Interface {}", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0]).toHaveProperty("implements");

    for (const lang of ["js", "jsx"]) {
      const standard = parse("class Standard implements Interface {}", { lang });
      expect(standard.diagnostics, lang).not.toEqual([]);
      expect(standard.program.body[0], lang).not.toHaveProperty("implements");
    }
  });

  it("materializes TypeScript generic classes without widening standard classes", () => {
    const source = [
      "class Generic<T extends Constraint = Fallback> {}",
      "class Derived<Key, Value = Key> extends Base implements Repository<Key, Value> {}",
      "const Anonymous = class<Item> {};",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [generic, derived, anonymousDeclaration] = result.program.body;
    expect(generic).toMatchObject({
      type: "ClassDeclaration",
      id: { name: "Generic" },
      typeParameters: {
        type: "TSTypeParameterDeclaration",
        params: [{
          name: { name: "T" },
          constraint: { typeName: { name: "Constraint" } },
          default: { typeName: { name: "Fallback" } },
        }],
      },
    });
    expect(generic).not.toHaveProperty("implements");
    expect(source.slice(generic.typeParameters.start, generic.typeParameters.end)).toBe(
      "<T extends Constraint = Fallback>",
    );
    expect(generic.typeParameters.range).toEqual([
      generic.typeParameters.start,
      generic.typeParameters.end,
    ]);
    expect(derived).toMatchObject({
      type: "ClassDeclaration",
      superClass: { name: "Base" },
      typeParameters: { params: [{ name: { name: "Key" } }, { name: { name: "Value" } }] },
      implements: [{
        expression: { name: "Repository" },
        typeArguments: { params: [{ typeName: { name: "Key" } }, { typeName: { name: "Value" } }] },
      }],
    });
    expect(anonymousDeclaration.declarations[0].init).toMatchObject({
      type: "ClassExpression",
      id: null,
      typeParameters: { params: [{ name: { name: "Item" } }] },
    });
    expect(anonymousDeclaration.declarations[0].init).not.toHaveProperty("implements");

    for (const lang of ["ts", "tsx", "dts"]) {
      const typed = parse("class Generic<T> {}", { lang });
      expect(typed.diagnostics, lang).toEqual([]);
      expect(typed.program.body[0]).toHaveProperty("typeParameters");
    }

    const compatibility = parse("class Generic<T> {}", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0]).toHaveProperty("typeParameters");

    for (const lang of ["js", "jsx"]) {
      const standard = parse("class Generic<T> {}", { lang });
      expect(standard.diagnostics, lang).not.toEqual([]);
      expect(standard.program.body[0], lang).not.toHaveProperty("typeParameters");
    }

    const empty = parse("class Empty<> {}", { lang: "ts" });
    expect(empty.diagnostics).not.toEqual([]);
    expect(empty.program.body[0].typeParameters.params).toEqual([]);
  });

  it("materializes optional TypeScript value parameters", () => {
    const source = [
      "function declared(required: Input, optional?: Output, inferred?) {}",
      "class Service { method(required: Input, optional?: Output) {} }",
      "const arrow = (required: Input, optional?: Output) => optional;",
      "const asyncArrow = async (required: Input, optional?: Output) => optional;",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    const [declaration, service, arrowDeclaration, asyncArrowDeclaration] = result.program.body;
    const parameterLists = [
      declaration.params,
      service.body.body[0].value.params,
      arrowDeclaration.declarations[0].init.params,
      asyncArrowDeclaration.declarations[0].init.params,
    ];

    for (const [required, optional] of parameterLists) {
      expect(required).toMatchObject({
        type: "Identifier",
        name: "required",
        optional: false,
        typeAnnotation: { type: "TSTypeAnnotation" },
      });
      expect(optional).toMatchObject({
        type: "Identifier",
        name: "optional",
        optional: true,
        typeAnnotation: { type: "TSTypeAnnotation" },
      });
    }
    expect(declaration.params[2]).toMatchObject({
      type: "Identifier",
      name: "inferred",
      optional: true,
      typeAnnotation: null,
    });
  });

  it("keeps optional parameter syntax out of JavaScript", () => {
    const result = parse("function invalid(value?: Input) {}", { lang: "js" });

    expect(result.diagnostics).not.toEqual([]);
    expect(result.program.type).toBe("Program");
  });

  it("keeps unsupported function return forms diagnostic", () => {
    for (
      const [source, options] of [
        ["function predicate(value: unknown): value is string { return true; }", { lang: "ts" }],
        ["function assertion(value: unknown): asserts value {}", { lang: "ts" }],
        ["function missing(): ; {}", { lang: "ts" }],
        ["function javascript(): string {}", { lang: "js" }],
      ]
    ) {
      const result = parse(source, options);
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("materializes TypeScript method return annotations", () => {
    const source = [
      "class Service {",
      "  method(): Namespace.Output {}",
      "  static [key](): Promise<Result> {}",
      "  #private(): Hidden {}",
      "  *values(): Iterable<Result> {}",
      "  async load(): Promise<Result> {}",
      "  get value(): Result {}",
      "  get #secret(): Hidden {}",
      "  plain() {}",
      "}",
      "const object = { method(): Result {}, get value(): Result {}, plain() {} };",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    const classMethods = result.program.body[0].body.body;
    const objectMethods = result.program.body[1].declarations[0].init.properties;
    for (const method of [...classMethods.slice(0, 7), ...objectMethods.slice(0, 2)]) {
      expect(method.value.returnType).toMatchObject({
        type: "TSTypeAnnotation",
        typeAnnotation: { type: expect.stringMatching(/^TS/) },
      });
    }
    expect(source.slice(classMethods[0].value.returnType.start, classMethods[0].value.returnType.end)).toBe(
      ": Namespace.Output",
    );
    expect(classMethods[0].value).toMatchObject({ async: false, generator: false });
    expect(classMethods[3].value).toMatchObject({ generator: true });
    expect(classMethods[4].value).toMatchObject({ async: true });
    expect(classMethods[7].value).not.toHaveProperty("returnType");
    expect(objectMethods[2].value).not.toHaveProperty("returnType");
  });

  it("materializes ESTree bodyless class signatures and constructor kinds", () => {
    const source = [
      "class Service extends Base {",
      "  constructor(value: Input);",
      "  constructor(value: Input) { super(); }",
      "  method(required: Input, fallback = value, ...rest): Result;",
      "  static [key](value = fallback): Output;",
      "  #private(value: Input): Hidden;",
      "  implemented() {}",
      "  static constructor() {}",
      "  ['constructor']() {}",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [
      declaredConstructor,
      constructor,
      method,
      computed,
      privateMethod,
      implemented,
      staticConstructor,
      computedConstructor,
    ] = result.program.body[0].body.body;
    expect(declaredConstructor).toMatchObject({
      type: "MethodDefinition",
      kind: "constructor",
      computed: false,
      static: false,
      value: {
        type: "TSEmptyBodyFunctionExpression",
        id: null,
        params: [{ name: "value", typeAnnotation: { type: "TSTypeAnnotation" } }],
        body: null,
        generator: false,
        async: false,
        expression: false,
        declare: false,
      },
    });
    expect(declaredConstructor.value).not.toHaveProperty("returnType");
    expect(source.slice(declaredConstructor.value.start, declaredConstructor.value.end)).toBe(
      "(value: Input);",
    );
    expect(declaredConstructor.value.range).toEqual([
      declaredConstructor.value.start,
      declaredConstructor.value.end,
    ]);
    expect(constructor).toMatchObject({
      type: "MethodDefinition",
      kind: "constructor",
      value: { type: "FunctionExpression", body: { type: "BlockStatement" } },
    });
    expect(method).toMatchObject({
      kind: "method",
      value: {
        type: "TSEmptyBodyFunctionExpression",
        params: [{}, {}, { type: "RestElement" }],
        returnType: { typeAnnotation: { typeName: { name: "Result" } } },
      },
    });
    expect(computed).toMatchObject({
      computed: true,
      static: true,
      value: { type: "TSEmptyBodyFunctionExpression" },
    });
    expect(privateMethod).toMatchObject({
      key: { type: "PrivateIdentifier", name: "private" },
      value: { type: "TSEmptyBodyFunctionExpression" },
    });
    expect(implemented).toMatchObject({
      kind: "method",
      value: { type: "FunctionExpression", body: { type: "BlockStatement" } },
    });
    expect(staticConstructor).toMatchObject({ kind: "method", static: true });
    expect(computedConstructor).toMatchObject({ kind: "method", computed: true });

    for (const lang of ["ts", "tsx", "dts"]) {
      const typed = parse("class C { method(); }", { lang });
      expect(typed.diagnostics, lang).toEqual([]);
      expect(typed.program.body[0].body.body[0].value.type).toBe("TSEmptyBodyFunctionExpression");
    }
    const compatibility = parse("class C { method(); }", { typescriptJsCompatibility: true });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0].body.body[0].value.type).toBe("TSEmptyBodyFunctionExpression");

    for (const lang of ["js", "jsx"]) {
      const standard = parse("class C { method(); }", { lang });
      expect(standard.diagnostics, lang).not.toEqual([]);
      expect(JSON.stringify(standard.program), lang).not.toContain("TSEmptyBodyFunctionExpression");
    }

    const objectMethod = parse("const value = { method(); };", { lang: "ts" });
    expect(objectMethod.diagnostics).not.toEqual([]);
    expect(JSON.stringify(objectMethod.program)).not.toContain("TSEmptyBodyFunctionExpression");
  });

  it("materializes ESTree TypeScript class member modifiers", () => {
    const source = [
      "class Base {}",
      "class Derived extends Base {",
      "  public constructor() { super(); }",
      "  protected static declared(): Output;",
      "  private field: Input;",
      "  readonly value = initial;",
      "  public override method() {}",
      "  override readonly size = 1;",
      "  protected get item() { return this.field; }",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const members = result.program.body[1].body.body;
    expect(members).toMatchObject([
      { type: "MethodDefinition", kind: "constructor", accessibility: "public" },
      {
        type: "MethodDefinition",
        kind: "method",
        accessibility: "protected",
        static: true,
        value: { type: "TSEmptyBodyFunctionExpression" },
      },
      { type: "PropertyDefinition", accessibility: "private" },
      { type: "PropertyDefinition", readonly: true },
      { type: "MethodDefinition", accessibility: "public", override: true },
      { type: "PropertyDefinition", override: true, readonly: true },
      { type: "MethodDefinition", kind: "get", accessibility: "protected" },
    ]);
    expect(members[0]).not.toHaveProperty("override");
    expect(members[2]).not.toHaveProperty("readonly");
    expect(members[3]).not.toHaveProperty("accessibility");
    for (const member of members) expect(member.range).toEqual([member.start, member.end]);
  });

  it("materializes ESTree abstract classes and members", () => {
    const source = [
      "abstract class Base<T> {}",
      "export abstract class Derived<T> extends Base implements Contract<T> {",
      "  public abstract method(value: T): T;",
      "  abstract readonly field: T;",
      "  abstract #privateMethod(): void;",
      "}",
      "export default abstract class {}",
    ].join("\n");
    const result = parse(source, {
      lang: "ts",
      range: true,
      sourceType: "module",
    });

    expect(result.diagnostics).toEqual([]);
    const base = result.program.body[0];
    const derived = result.program.body[1].declaration;
    const exportedDefault = result.program.body[2].declaration;
    expect(base).toMatchObject({ type: "ClassDeclaration", abstract: true });
    expect(derived).toMatchObject({
      type: "ClassDeclaration",
      abstract: true,
      typeParameters: { type: "TSTypeParameterDeclaration" },
      implements: [{ type: "TSClassImplements" }],
    });
    expect(exportedDefault).toMatchObject({ type: "ClassDeclaration", abstract: true });
    expect(derived.body.body).toMatchObject([
      {
        type: "TSAbstractMethodDefinition",
        accessibility: "public",
        value: { type: "TSEmptyBodyFunctionExpression", body: null },
      },
      { type: "TSAbstractPropertyDefinition", readonly: true, value: null },
      {
        type: "TSAbstractMethodDefinition",
        key: { type: "PrivateIdentifier", name: "privateMethod" },
      },
    ]);
    for (const node of [base, derived, exportedDefault, ...derived.body.body]) {
      expect(node.range).toEqual([node.start, node.end]);
      if (node.type.startsWith("TSAbstract")) expect(node).not.toHaveProperty("abstract");
    }
  });

  it("keeps abstract accessor and async-generator signatures separate from following members", () => {
    const result = parse(
      [
        "abstract class Signatures {",
        "  abstract get value(): string;",
        "  abstract set value(next: string);",
        "  abstract async load(): Promise<string>;",
        "  abstract *values(): IterableIterator<string>;",
        "  after: string;",
        "}",
      ].join("\n"),
      { lang: "ts", semanticErrors: true },
    );
    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].body.body).toMatchObject([
      {
        type: "TSAbstractMethodDefinition",
        kind: "get",
        value: { type: "TSEmptyBodyFunctionExpression", generator: false, async: false },
      },
      {
        type: "TSAbstractMethodDefinition",
        kind: "set",
        value: { type: "TSEmptyBodyFunctionExpression", generator: false, async: false },
      },
      {
        type: "TSAbstractMethodDefinition",
        kind: "method",
        value: { type: "TSEmptyBodyFunctionExpression", generator: false, async: true },
      },
      {
        type: "TSAbstractMethodDefinition",
        kind: "method",
        value: { type: "TSEmptyBodyFunctionExpression", generator: true, async: false },
      },
      { type: "PropertyDefinition", key: { name: "after" } },
    ]);
  });

  it("keeps abstract contextual and diagnoses invalid abstract members", () => {
    const contextual = parse(
      [
        "abstract",
        "class Ordinary {}",
        "abstract as Type;",
        "abstract satisfies Type;",
        "export default abstract;",
        "class Names { abstract(); abstract!: void; abstract\nmethod(); }",
      ].join("\n"),
      { lang: "ts", sourceType: "module" },
    );
    expect(contextual.program.body[0]).toMatchObject({
      type: "ExpressionStatement",
      expression: { name: "abstract" },
    });
    expect(contextual.program.body[1]).toMatchObject({
      type: "ClassDeclaration",
      id: { name: "Ordinary" },
    });
    expect(contextual.program.body[1]).not.toHaveProperty("abstract");
    expect(JSON.stringify(contextual.program)).not.toContain("TSAbstract");

    for (
      const source of [
        "class C { abstract method(); }",
        "abstract class C { abstract method() {} }",
        "abstract class C { abstract property = 1; }",
        "abstract class C { static abstract method(); }",
        "abstract class C { abstract constructor(); }",
        "abstract class C { override abstract method(); }",
        "abstract class C { abstract #field: number; }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    const privateMethod = parse("abstract class C { abstract #method(): void; }", {
      lang: "ts",
      semanticErrors: true,
    });
    expect(privateMethod.diagnostics).toEqual([]);
    expect(privateMethod.program.body[0].body.body[0].type).toBe("TSAbstractMethodDefinition");
  });

  it("preserves modifier-shaped class member names and escaped modifier spellings", () => {
    const source = [
      "class Base {}",
      "class Names extends Base {",
      "  public() {}",
      "  private() {}",
      "  protected;",
      "  readonly = 0;",
      "  override() {}",
      "  public static() {}",
      "  override readonly() {}",
      "  static static() {}",
      "  p\\u0075blic escapedField;",
      "  r\\u0065adonly escapedReadonly;",
      "  ov\\u0065rride escapedOverride() {}",
      "  public",
      "  private() {}",
      "  static",
      "  readonly",
      "  protected() {}",
      "  async = 1;",
      "  async() {}",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[1].body.body).toMatchObject([
      { type: "MethodDefinition", key: { name: "public" } },
      { type: "MethodDefinition", key: { name: "private" } },
      { type: "PropertyDefinition", key: { name: "protected" } },
      { type: "PropertyDefinition", key: { name: "readonly" } },
      { type: "MethodDefinition", key: { name: "override" } },
      { type: "MethodDefinition", key: { name: "static" }, accessibility: "public" },
      { type: "MethodDefinition", key: { name: "readonly" }, override: true },
      { type: "MethodDefinition", key: { name: "static" }, static: true },
      { type: "PropertyDefinition", key: { name: "escapedField" }, accessibility: "public" },
      { type: "PropertyDefinition", key: { name: "escapedReadonly" }, readonly: true },
      { type: "MethodDefinition", key: { name: "escapedOverride" }, override: true },
      { type: "PropertyDefinition", key: { name: "public" } },
      { type: "MethodDefinition", key: { name: "private" } },
      { type: "PropertyDefinition", key: { name: "readonly" }, static: true },
      { type: "MethodDefinition", key: { name: "protected" } },
      { type: "PropertyDefinition", key: { name: "async" } },
      { type: "MethodDefinition", key: { name: "async" } },
    ]);
  });

  it("gates and diagnoses TypeScript class member modifiers", () => {
    const compatibility = parse("class C { public field; protected method() {} }", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0].body.body).toMatchObject([
      { type: "PropertyDefinition", accessibility: "public" },
      { type: "MethodDefinition", accessibility: "protected" },
    ]);

    for (const lang of ["js", "jsx"]) {
      const gated = parse("class C { public field; protected method() {} }", { lang });
      expect(gated.diagnostics).not.toEqual([]);
      for (const member of gated.program.body[0].body.body) {
        expect(member).not.toHaveProperty("accessibility");
      }
    }

    for (
      const source of [
        "class C { readonly method() {} }",
        "class C extends B { override constructor() {} }",
        "class C { override method() {} }",
        "class C { public #field; }",
        "class C { public static {} }",
        "class C { readonly public field; }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("materializes TypeScript declared variables and type-only exports", () => {
    const source = [
      "declare var first;",
      "declare let second: string;",
      "declare const third: number;",
      "export declare var exportedFirst;",
      "export declare let exportedSecond: string;",
      "export declare const exportedThird: number;",
    ].join("\n");
    const result = parse(source, { lang: "ts", sourceType: "module", range: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body.slice(0, 3)).toMatchObject([
      { type: "VariableDeclaration", kind: "var", declare: true },
      { type: "VariableDeclaration", kind: "let", declare: true },
      { type: "VariableDeclaration", kind: "const", declare: true },
    ]);
    expect(result.program.body.slice(3)).toMatchObject([
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "VariableDeclaration", kind: "var", declare: true },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "VariableDeclaration", kind: "let", declare: true },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "VariableDeclaration", kind: "const", declare: true },
      },
    ]);
    for (const statement of result.program.body) {
      const declaration = statement.declaration ?? statement;
      expect(source.slice(declaration.start, declaration.start + 7)).toBe("declare");
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }

    const ordinary = parse("var first; let second; const third = 0;", { lang: "ts" });
    expect(ordinary.diagnostics).toEqual([]);
    for (const declaration of ordinary.program.body) {
      expect(declaration).not.toHaveProperty("declare");
    }
  });

  it("keeps declare-variable syntax contextual and TypeScript-only", () => {
    for (
      const source of [
        "declare; var value;",
        "declare\nvar value;",
        "declar\\u0065 var value;",
        "declare v\\u0061r value;",
        "export declare\nvar value;",
      ]
    ) {
      const result = parse(source, { lang: "ts", sourceType: "module" });
      expect(JSON.stringify(result.program), source).not.toContain("\"declare\":true");
    }

    const exported = parse("export\ndeclare var value;", {
      lang: "ts",
      sourceType: "module",
    });
    expect(exported.diagnostics).toEqual([]);
    expect(exported.program.body[0]).toMatchObject({
      type: "ExportNamedDeclaration",
      exportKind: "type",
      declaration: { type: "VariableDeclaration", declare: true },
    });

    for (const lang of ["js", "jsx"]) {
      const result = parse("declare var value;", { lang });
      expect(result.diagnostics, lang).not.toEqual([]);
      expect(JSON.stringify(result.program), lang).not.toContain("\"declare\":true");
    }
    const compatibility = parse("declare var value;", { typescriptJsCompatibility: true });
    expect(compatibility.diagnostics).not.toEqual([]);
    expect(JSON.stringify(compatibility.program)).not.toContain("\"declare\":true");
  });

  it("materializes explicit TypeScript declared enums and type-only exports", () => {
    const source = [
      "declare enum Direction { Up, Down = 2 }",
      "declare const",
      "enum ConstantDirection { Up = calculate() }",
      "export declare enum ExportedDirection { Up }",
      "export declare const enum ExportedConstantDirection { Up }",
      "enum OrdinaryDirection { Up }",
    ].join("\n");
    const result = parse(source, { lang: "ts", sourceType: "module", range: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "TSEnumDeclaration",
        id: { name: "Direction" },
        const: false,
        declare: true,
        body: {
          members: [
            { id: { name: "Up" }, initializer: null },
            { id: { name: "Down" }, initializer: { type: "Literal", value: 2 } },
          ],
        },
      },
      {
        type: "TSEnumDeclaration",
        id: { name: "ConstantDirection" },
        const: true,
        declare: true,
        body: {
          members: [{ initializer: { type: "CallExpression", callee: { name: "calculate" } } }],
        },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: {
          type: "TSEnumDeclaration",
          id: { name: "ExportedDirection" },
          const: false,
          declare: true,
        },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: {
          type: "TSEnumDeclaration",
          id: { name: "ExportedConstantDirection" },
          const: true,
          declare: true,
        },
      },
      {
        type: "TSEnumDeclaration",
        id: { name: "OrdinaryDirection" },
        const: false,
        declare: false,
      },
    ]);

    for (const statement of result.program.body.slice(0, 4)) {
      const declaration = statement.declaration ?? statement;
      expect(source.slice(declaration.start, declaration.start + 7)).toBe("declare");
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
      if (statement.declaration) {
        expect(source.slice(statement.start, statement.start + 6)).toBe("export");
        expect(statement.range).toEqual([statement.start, statement.end]);
      }
    }
  });

  it("keeps explicit declared enums contextual and TypeScript-only", () => {
    for (const lang of ["ts", "tsx", "dts"]) {
      const result = parse("declare enum Choice { First }", { lang });
      expect(result.diagnostics, lang).toEqual([]);
      expect(result.program.body[0]).toMatchObject({
        type: "TSEnumDeclaration",
        declare: true,
      });
    }

    for (
      const source of [
        "declare\nenum Choice {}",
        "declar\\u0065 enum Choice {}",
        "declare en\\u0075m Choice {}",
        "declare c\\u006fnst enum Choice {}",
        "declare const en\\u0075m Choice {}",
        "export declare\nenum Choice {}",
      ]
    ) {
      const result = parse(source, { lang: "ts", sourceType: "module" });
      const enums = result.program.body
        .map(statement => statement.declaration ?? statement)
        .filter(statement => statement.type === "TSEnumDeclaration");
      expect(enums.every(declaration => declaration.declare === false), source).toBe(true);
    }

    for (
      const options of [
        { lang: "js" },
        { lang: "jsx" },
        { typescriptJsCompatibility: true },
      ]
    ) {
      const result = parse("declare enum Choice {}", options);
      expect(result.diagnostics).not.toEqual([]);
      const enums = result.program.body.filter(statement => statement.type === "TSEnumDeclaration");
      expect(enums.every(declaration => declaration.declare === false)).toBe(true);
    }
  });

  it("materializes explicit TypeScript declared namespaces", () => {
    const source = [
      "declare namespace N\\u0061me.default { namespace Inner { const value: number; } declare namespace Explicit {} }",
      "export\ndeclare namespace Public {}",
      "namespace Ordinary {}",
    ].join("\n");
    const result = parse(source, { lang: "ts", sourceType: "module", range: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "TSModuleDeclaration",
        declare: true,
        kind: "namespace",
        id: {
          type: "TSQualifiedName",
          left: { type: "Identifier", name: "Name" },
          right: { type: "Identifier", name: "default" },
        },
        body: {
          body: [
            { type: "TSModuleDeclaration", declare: false, id: { name: "Inner" } },
            { type: "TSModuleDeclaration", declare: true, id: { name: "Explicit" } },
          ],
        },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: {
          type: "TSModuleDeclaration",
          declare: true,
          id: { name: "Public" },
        },
      },
      {
        type: "TSModuleDeclaration",
        declare: false,
        id: { name: "Ordinary" },
      },
    ]);
    const declared = [result.program.body[0], result.program.body[1].declaration];
    for (const declaration of declared) {
      expect(source.slice(declaration.start, declaration.start + 7)).toBe("declare");
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }
    expect(source.slice(result.program.body[1].start, result.program.body[1].start + 6)).toBe("export");
  });

  it("materializes explicit ambient external modules and global augmentations", () => {
    const source = [
      "declare module \"package\" { import value from \"dependency\"; export { value } from \"dependency\"; }",
      "declare module \"empty\";",
      "declare global\n{ let shared: number; }",
      "export declare module \"exported\" {}",
      "export declare global {}",
    ].join("\n");
    const result = parse(source, {
      lang: "ts",
      sourceType: "module",
      semanticErrors: true,
      range: true,
    });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "TSModuleDeclaration",
        declare: true,
        kind: "module",
        id: { type: "Literal", value: "package", raw: "\"package\"" },
        body: {
          type: "TSModuleBlock",
          body: [
            { type: "ImportDeclaration" },
            { type: "ExportNamedDeclaration", source: { value: "dependency" } },
          ],
        },
      },
      {
        type: "TSModuleDeclaration",
        declare: true,
        kind: "module",
        id: { value: "empty" },
        body: null,
      },
      {
        type: "TSModuleDeclaration",
        declare: true,
        kind: "global",
        id: { type: "Identifier", name: "global" },
        body: { type: "TSModuleBlock", body: [{ type: "VariableDeclaration" }] },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "TSModuleDeclaration", kind: "module", declare: true },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "TSModuleDeclaration", kind: "global", declare: true },
      },
    ]);
    for (
      const declaration of [
        result.program.body[0],
        result.program.body[1],
        result.program.body[2],
        result.program.body[3].declaration,
        result.program.body[4].declaration,
      ]
    ) {
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }
  });

  it("materializes contextual global augmentations with transparent bindings", () => {
    const source = [
      "let topLevel: string; global\n{ let topLevel: number; function topImplementation() {} class Top { method() {} field = 1; } let topInitializer = 1; }",
      "declare module \"ambient\" { let nested: string; global { let nested: number; function ambientImplementation() {} class Ambient { method() {} field = 1; } let ambientInitializer = 1; } }",
      "namespace Ordinary { global { function namespaceImplementation() {} class Nested { method() {} field = 1; } let namespaceInitializer = 1; } }",
    ].join("\n");
    const result = parse(source, { lang: "ts", semanticErrors: true, range: true });

    const ambientDiagnostics = [
      "function implementations are not allowed in ambient contexts",
      "class method implementations are not allowed in ambient contexts",
      "class property initializers are not allowed in ambient contexts",
      "initializers are not allowed in ambient contexts",
    ];
    expect(result.diagnostics).toEqual(expect.arrayContaining([
      "duplicate binding `topLevel`",
      "duplicate binding `nested`",
      ...ambientDiagnostics,
    ]));
    expect(result.diagnostics.filter(diagnostic => ambientDiagnostics.includes(diagnostic))).toEqual(
      ambientDiagnostics,
    );
    const topLevelGlobal = result.program.body[1];
    const externalGlobal = result.program.body[2].body.body[1];
    const namespaceGlobal = result.program.body[3].body.body[0];
    for (const declaration of [topLevelGlobal, externalGlobal, namespaceGlobal]) {
      expect(declaration).toMatchObject({
        type: "TSModuleDeclaration",
        id: { type: "Identifier", name: "global" },
        body: { type: "TSModuleBlock" },
        declare: false,
        kind: "global",
      });
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }

    const placed = parse(
      "function f() { global {} } { global {} } declare global { global {} }",
      { lang: "ts", semanticErrors: true },
    );
    expect(placed.diagnostics).toEqual([]);
    expect([
      placed.program.body[0].body.body[0],
      placed.program.body[1].body[0],
      placed.program.body[2],
      placed.program.body[2].body.body[0],
    ]).toMatchObject([
      { type: "TSModuleDeclaration", kind: "global", declare: false },
      { type: "TSModuleDeclaration", kind: "global", declare: false },
      { type: "TSModuleDeclaration", kind: "global", declare: true },
      { type: "TSModuleDeclaration", kind: "global", declare: false },
    ]);

    for (const source of ["gl\\u006fbal {}", "global;", "global\n;"]) {
      const recovered = parse(source, { lang: "ts" });
      expect(JSON.stringify(recovered.program), source).not.toContain("TSModuleDeclaration");
    }

    for (
      const options of [
        { lang: "js" },
        { lang: "jsx" },
        { typescriptJsCompatibility: true },
      ]
    ) {
      const recovered = parse("global {}", options);
      expect(JSON.stringify(recovered.program)).not.toContain("TSModuleDeclaration");
    }
  });

  it("diagnoses labeled statements whose label is not an identifier", () => {
    for (
      const source of [
        "this.property: value;",
        "(label): value;",
        "class B { constructor() { this.y: any; } }",
      ]
    ) {
      const result = parse(source, { lang: source.startsWith("class") ? "ts" : "js" });
      expect(result.diagnostics, source).toContain(
        "labeled statement requires an identifier label",
      );
    }

    expect(parse("label: value;", { lang: "js" }).diagnostics).toEqual([]);
  });

  it("recovers ambient module heads and preserves scope-specific diagnostics", () => {
    const semanticLegacy = parse("declare module Legacy.Deep {}", {
      lang: "ts",
      semanticErrors: true,
    });
    expect(semanticLegacy.program.body[0]).toMatchObject({
      type: "TSModuleDeclaration",
      kind: "module",
      declare: true,
      id: { type: "TSQualifiedName" },
    });
    expect(semanticLegacy.diagnostics).toEqual([
      "ambient external module name must be a string literal",
    ]);

    for (const source of ["declare module 42 {}", "declare module {}"]) {
      const result = parse(source, { lang: "ts" });
      expect(result.program.body[0]).toMatchObject({
        type: "TSModuleDeclaration",
        kind: "module",
        declare: true,
      });
      expect(result.diagnostics).toContain(
        "ambient module name must be a string literal or identifier",
      );
      if (source === "declare module {}") {
        expect(result.program.body[0].id).toBeNull();
      }
    }
    const bodylessGlobal = parse("declare global;", { lang: "ts" });
    expect(bodylessGlobal.program.body[0]).toMatchObject({
      type: "TSModuleDeclaration",
      kind: "global",
      declare: true,
    });
    expect(bodylessGlobal.diagnostics).toEqual([
      "global augmentation requires a module block",
    ]);

    const scoped = parse(
      "declare module \"outer\" { import value from \"dependency\"; namespace Inner { import nested from \"dependency\"; export * from \"dependency\"; } } declare global { let shared: number; function implemented() {} class C { method() {} } } let shared: number; declare global { declare global {} }",
      { lang: "ts", semanticErrors: true },
    );
    expect(scoped.diagnostics).toEqual(expect.arrayContaining([
      "import declarations in a namespace cannot reference a module",
      "export-all declarations are not allowed in internal namespaces",
      "duplicate binding `shared`",
      "function implementations are not allowed in ambient contexts",
      "class method implementations are not allowed in ambient contexts",
      "global augmentations are only allowed at the top level of a namespace or module",
    ]));

    const namespaceExportCollision = parse(
      "export as namespace exportedGlobal; declare global { export let exportedGlobal; }",
      { lang: "ts", sourceType: "module", semanticErrors: true },
    );
    expect(namespaceExportCollision.diagnostics).toEqual([
      "duplicate binding `exportedGlobal`",
    ]);

    for (
      const source of [
        "declare\nmodule \"split\" {}",
        "declare module\n\"split\" {}",
        "declar\\u0065 module \"escaped\" {}",
        "declare mod\\u0075le \"escaped\" {}",
        "declare gl\\u006fbal {}",
        "global {}",
      ]
    ) {
      const result = parse(source, { lang: "ts" });
      expect(JSON.stringify(result.program), source).not.toContain("\"declare\":true");
    }
  });

  it("keeps explicit declared namespaces contextual and TypeScript-only", () => {
    for (const lang of ["ts", "tsx", "dts"]) {
      const result = parse("declare namespace Included {}", { lang });
      expect(result.diagnostics, lang).toEqual([]);
      expect(result.program.body[0]).toMatchObject({
        type: "TSModuleDeclaration",
        declare: true,
      });
    }

    for (
      const source of [
        "declare namespace\nSeparated {}",
        "declar\\u0065 namespace Escaped {}",
        "declare namesp\\u0061ce Escaped {}",
        "declare namespace default.Name {}",
        "declare namespace enum.Name {}",
        "declare namespace {}",
        "declare.namespace;",
        "declare: namespace;",
      ]
    ) {
      const result = parse(source, { lang: "ts" });
      expect(JSON.stringify(result.program), source).not.toContain("\"declare\":true");
    }

    const separated = parse("declare\nnamespace Ordinary {}", { lang: "ts" });
    expect(separated.program.body.at(-1)).toMatchObject({
      type: "TSModuleDeclaration",
      declare: false,
    });
    for (const source of ["namespace\nName {}", "module\nName {}"]) {
      const result = parse(source, { lang: "ts" });
      expect(JSON.stringify(result.program), source).not.toContain("TSModuleDeclaration");
    }

    const semanticFree = parse("\"use strict\"; namespace public {}", {
      lang: "ts",
      semanticErrors: false,
    });
    expect(semanticFree.diagnostics).toEqual([]);
    expect(semanticFree.program.body.at(-1)).toMatchObject({
      type: "TSModuleDeclaration",
      id: { name: "public" },
    });

    const ambientSloppy = parse(
      "declare namespace N { function eval(): void; function arguments(): void; class C { method(eval: unknown): void; method2(arguments: unknown): void; } }",
      { lang: "ts", semanticErrors: true, sourceType: "module" },
    );
    expect(ambientSloppy.diagnostics).toEqual([]);
    const restored = parse("declare namespace N {} function eval() {}", {
      lang: "ts",
      semanticErrors: true,
      sourceType: "module",
    });
    expect(restored.diagnostics).not.toEqual([]);

    for (
      const source of [
        "function f() { declare namespace N {} }",
        "if (condition) { declare namespace N {} }",
      ]
    ) {
      const misplaced = parse(source, { lang: "ts", semanticErrors: true });
      expect(misplaced.diagnostics, source).not.toEqual([]);
    }

    for (
      const options of [
        { lang: "js" },
        { lang: "jsx" },
        { typescriptJsCompatibility: true },
      ]
    ) {
      const result = parse("declare namespace Excluded {}", options);
      expect(result.diagnostics).not.toEqual([]);
      expect(JSON.stringify(result.program)).not.toContain("TSModuleDeclaration");
    }
  });

  it("enforces ambient namespace declarations and internal exports", () => {
    const valid = parse(
      "declare namespace Valid { function signature(): void; class C { method(): void; rest(...items: any[],): void; get value(): string; field: string; readonly inferred = Symbol(); } var value: number; let later: string; const text = 'value'; const truth = true; const count = -1; const large = -1n; const template = `value`; const member = Enum.Member; const keyword = Enum.default; const indexed = Enum['Member']; const templated = Namespace.Enum[`Member`]; namespace Nested { const value: number; } export interface Item {} export { value }; } function outside() {} class Outside { field = 1; method() {} } const runtime = 1 + 2; export default runtime;",
      { lang: "ts", semanticErrors: true },
    );
    expect(valid.diagnostics).toEqual([]);

    for (
      const source of [
        "declare namespace N { function implemented() {} }",
        "declare namespace N { class C { method() {} } }",
        "declare namespace N { class C { get value() { return 1; } } }",
        "declare namespace N { class C { constructor() {} } }",
        "declare namespace N { class C { static {} } }",
        "declare namespace N { class C { field = 1; } }",
        "declare namespace N { class C { readonly typed: number = 1; } }",
        "declare namespace N { var value = 1; }",
        "declare namespace N { let value: number = 1; }",
        "declare namespace N { const value: number = 1; }",
        "declare namespace N { const value = (1); }",
        "declare namespace N { const value = null; }",
        "declare namespace N { const value = 1 + 2; }",
        "declare namespace N { const value = Namespace[member]; }",
        "declare namespace N { export = N; }",
        "declare namespace N { export as namespace N; }",
        "declare namespace N { export default value; }",
        "declare namespace N { export { value } from 'module'; }",
        "declare namespace N { export * from 'module'; }",
        "declare namespace N { export * as values from 'module'; }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("materializes top-level TypeScript overload signatures", () => {
    const source = [
      "export function overloaded<T>(value: T): T;",
      "export function overloaded(value: string): string;",
      "function overloaded(value) { return value; }",
      "function following(): void {}",
      "export { overloaded };",
    ].join("\n");
    const result = parse(source, { lang: "ts", sourceType: "module", range: true, semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const first = result.program.body[0].declaration;
    const second = result.program.body[1].declaration;
    expect(first).toMatchObject({
      type: "TSDeclareFunction",
      id: { type: "Identifier", name: "overloaded" },
      params: [{
        type: "Identifier",
        name: "value",
        typeAnnotation: { typeAnnotation: { type: "TSTypeReference" } },
      }],
      generator: false,
      async: false,
      returnType: { typeAnnotation: { type: "TSTypeReference" } },
      typeParameters: {
        type: "TSTypeParameterDeclaration",
        params: [{ name: { name: "T" } }],
      },
    });
    expect(first).not.toHaveProperty("body");
    expect(first).not.toHaveProperty("declare");
    expect(source.slice(first.start, first.end)).toBe("function overloaded<T>(value: T): T;");
    expect(first.range).toEqual([first.start, first.end]);
    expect(second).toMatchObject({
      type: "TSDeclareFunction",
      returnType: { typeAnnotation: { type: "TSStringKeyword" } },
    });
    expect(second).not.toHaveProperty("typeParameters");
    expect(result.program.body[2]).toMatchObject({
      type: "FunctionDeclaration",
      body: { type: "BlockStatement" },
    });
    expect(result.program.body[3]).toMatchObject({
      type: "FunctionDeclaration",
      returnType: { typeAnnotation: { type: "TSVoidKeyword" } },
    });
    expect(result.program.body[4]).toMatchObject({
      type: "ExportNamedDeclaration",
      specifiers: [{ local: { name: "overloaded" } }],
    });

    for (const lang of ["ts", "tsx", "dts"]) {
      const typed = parse("function signature(value: Input): Output;", { lang });
      expect(typed.diagnostics, lang).toEqual([]);
      expect(typed.program.body[0].type).toBe("TSDeclareFunction");
    }
    const compatibility = parse("function signature();", { typescriptJsCompatibility: true });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0].type).toBe("TSDeclareFunction");

    const extended = parse(
      [
        "async function asynchronous(): Promise<void>;",
        "function* generator(): Iterable<void>;",
        "function outer() { function nested(): void; }",
        "function lineBreak(): void",
        "function following() {}",
        "function eof(): void",
      ].join("\n"),
      { lang: "ts" },
    );
    expect(extended.diagnostics).toEqual([]);
    expect(extended.program.body[0]).toMatchObject({
      type: "TSDeclareFunction",
      async: true,
      generator: false,
    });
    expect(extended.program.body[1]).toMatchObject({
      type: "TSDeclareFunction",
      async: false,
      generator: true,
    });
    expect(extended.program.body[2].body.body[0].type).toBe("TSDeclareFunction");
    expect(extended.program.body[3].type).toBe("TSDeclareFunction");
    expect(extended.program.body[5].type).toBe("TSDeclareFunction");

    for (
      const [source, options] of [
        ["function signature();", { lang: "js" }],
        ["function signature();", { lang: "jsx" }],
        ["const expression = function named(): void;", { lang: "ts" }],
        ["function signature(): void const value = 1;", { lang: "ts" }],
      ]
    ) {
      const excluded = parse(source, options);
      expect(excluded.diagnostics, source).not.toEqual([]);
      expect(JSON.stringify(excluded.program), source).not.toContain("TSDeclareFunction");
    }
  });

  it("materializes explicit TypeScript declared functions", () => {
    const source = [
      "declare function plain(value: string): void;",
      "declare function* generated<T>(...values: T[],): Iterable<T>",
      "declare async function asynchronous(): Promise<void>;",
      "declare async function* asynchronousGenerator<T>(): AsyncIterable<T>;",
      "function outer() { declare function nested(): void; }",
      "export declare function exported<T>(): T;",
      "function overload(): void;",
      "declare function eof(): void",
    ].join("\n");
    const result = parse(source, {
      lang: "ts",
      range: true,
      sourceType: "module",
    });

    expect(result.diagnostics).toEqual([]);
    const explicit = [
      result.program.body[0],
      result.program.body[1],
      result.program.body[2],
      result.program.body[3],
      result.program.body[4].body.body[0],
      result.program.body[5].declaration,
      result.program.body[7],
    ];
    for (const declaration of explicit) {
      expect(declaration).toMatchObject({
        type: "TSDeclareFunction",
        declare: true,
      });
      expect(source.slice(declaration.start, declaration.start + 7)).toBe("declare");
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }
    expect(result.program.body[1]).toMatchObject({ generator: true, async: false });
    expect(result.program.body[2]).toMatchObject({ generator: false, async: true });
    expect(result.program.body[3]).toMatchObject({ generator: true, async: true });
    expect(result.program.body[5]).toMatchObject({
      type: "ExportNamedDeclaration",
      exportKind: "type",
    });
    expect(result.program.body[6]).toMatchObject({
      type: "TSDeclareFunction",
      id: { name: "overload" },
    });
    expect(result.program.body[6]).not.toHaveProperty("declare");
  });

  it("keeps explicit declared functions contextual and restores ambient grammar", () => {
    for (
      const source of [
        "declare\nfunction separated(): void;",
        "declare async\nfunction separated(): void;",
        "declar\\u0065 function escaped(): void;",
        "declare f\\u0075nction escaped(): void;",
        "declare as\\u0079nc function escaped(): void;",
        "export declare\nfunction separated(): void;",
      ]
    ) {
      const result = parse(source, { lang: "ts", sourceType: "module" });
      expect(JSON.stringify(result.program), source).not.toContain("\"declare\":true");
    }

    for (const lang of ["js", "jsx"]) {
      const result = parse("declare function excluded(): void;", { lang });
      expect(result.diagnostics, lang).not.toEqual([]);
      expect(JSON.stringify(result.program), lang).not.toContain("\"declare\":true");
    }
    const compatibility = parse("declare function excluded(): void;", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).not.toEqual([]);
    expect(JSON.stringify(compatibility.program)).not.toContain("\"declare\":true");

    const recovered = parse(
      "declare function initialized(value = 1): void; declare function implemented() {} function ordinary() {}",
      { lang: "ts", semanticErrors: true },
    );
    expect(recovered.diagnostics).toHaveLength(2);
    expect(recovered.program.body).toMatchObject([
      { type: "TSDeclareFunction", declare: true },
      { type: "TSDeclareFunction", declare: true, body: { type: "BlockStatement" } },
      { type: "FunctionDeclaration", body: { type: "BlockStatement" } },
    ]);

    const invalidModifiers = parse(
      "declare async function asynchronous(): void; declare function* generated(): void; declare async function* both(): void;",
      { lang: "ts", semanticErrors: true },
    );
    expect(invalidModifiers.diagnostics).toHaveLength(4);
    expect(invalidModifiers.diagnostics.filter((diagnostic) => diagnostic.includes("async functions"))).toHaveLength(2);
    expect(invalidModifiers.diagnostics.filter((diagnostic) => diagnostic.includes("generators"))).toHaveLength(2);

    const ambientNames = parse("declare function eval(arguments: unknown): void;", {
      lang: "ts",
      semanticErrors: true,
      sourceType: "module",
    });
    expect(ambientNames.diagnostics).toEqual([]);
    const restorationSource = "declare function eval(arguments: unknown): void; function arguments() {}";
    const restoration = parse(restorationSource, {
      lang: "ts",
      semanticErrors: true,
      sourceType: "module",
    });
    expect(restoration.diagnostics).not.toEqual([]);
  });

  it("permits rest trailing commas only in TypeScript signatures", () => {
    for (
      const source of [
        "declare function explicit(...values: unknown[], );",
        "function overload(...values: unknown[], ): void;",
      ]
    ) {
      expect(parse(source, { lang: "ts", semanticErrors: true }).diagnostics, source).toEqual([]);
    }
    for (
      const [source, options] of [
        ["function runtime(...values: unknown[], ) {}", { lang: "ts", semanticErrors: true }],
        ["function javascript(...values, ) {}", { semanticErrors: true }],
        ["class C { method(...values: unknown[], ): void; }", { lang: "ts", semanticErrors: true }],
      ]
    ) {
      expect(parse(source, options).diagnostics, source).not.toEqual([]);
    }
    for (
      const source of [
        "declare function explicit(...values: unknown[], ) {}",
        "declare namespace N { class C { method(...values: unknown[], ) {} } }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).toHaveLength(1);
      expect(result.diagnostics[0]).toContain("implementation");
    }
  });

  it("keeps unsupported method return forms diagnostic", () => {
    for (
      const [source, options] of [
        ["class C { predicate(value): value is string {} }", { lang: "ts" }],
        ["class C { assertion(value): asserts value {} }", { lang: "ts" }],
        ["class C { constructor(): string {} }", { lang: "ts" }],
        ["class C { method(): string {} }", { lang: "js" }],
        ["class C { method(): string {} }", { lang: "jsx" }],
      ]
    ) {
      const result = parse(source, options);
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("diagnoses typed setters only when semantic errors are enabled", () => {
    const source = [
      "class C { set value(next): void {} }",
      "const object = { set value(next): void {} };",
    ].join("\n");
    const syntaxOnly = parse(source, { lang: "ts" });
    const semantic = parse(source, { lang: "ts", semanticErrors: true });

    expect(syntaxOnly.diagnostics).toEqual([]);
    const syntaxOnlySetters = [
      syntaxOnly.program.body[0].body.body[0].value,
      syntaxOnly.program.body[1].declarations[0].init.properties[0].value,
    ];
    for (const setter of syntaxOnlySetters) {
      expect(setter.returnType).toMatchObject({
        type: "TSTypeAnnotation",
        typeAnnotation: { type: "TSVoidKeyword" },
      });
    }
    expect(semantic.diagnostics).not.toEqual([]);
    expect(semantic.program.body[0].body.body[0].value.returnType).toMatchObject({ type: "TSTypeAnnotation" });
    expect(semantic.program.body[1].declarations[0].init.properties[0].value.returnType).toMatchObject({
      type: "TSTypeAnnotation",
    });
  });

  it("requires super to continue as a call or property", () => {
    const invalid = parse("class C extends Base { method(): void { super; } }", { lang: "ts" });
    const valid = parse(
      "class C extends Base { constructor() { super(); } method() { super.value; super[key]; } }",
      { lang: "ts" },
    );

    expect(invalid.diagnostics).not.toEqual([]);
    expect(invalid.program.type).toBe("Program");
    expect(valid.diagnostics).toEqual([]);
  });

  it("does not join exported async functions across a line break", () => {
    const named = parse("export async/*\n*/function split() {}");
    expect(named.diagnostics).not.toEqual([]);
    expect(named.program.type).toBe("Program");

    const defaultExport = parse("export default async\nfunction split() {}");
    expect(defaultExport.diagnostics).toEqual([]);
    expect(defaultExport.program.body).toMatchObject([
      {
        type: "ExportDefaultDeclaration",
        declaration: { type: "Identifier", name: "async" },
      },
      {
        type: "FunctionDeclaration",
        generator: false,
        async: false,
      },
    ]);
  });

  it("recovers malformed exported async functions", () => {
    const named = parse("export async function broken(");
    const defaultExport = parse("export default async function* broken(");

    expect(named.diagnostics).not.toEqual([]);
    expect(named.program.body[0]).toMatchObject({
      type: "ExportNamedDeclaration",
      declaration: {
        type: "FunctionDeclaration",
        generator: false,
        async: true,
      },
    });
    expect(defaultExport.diagnostics).not.toEqual([]);
    expect(defaultExport.program.body[0]).toMatchObject({
      type: "ExportDefaultDeclaration",
      declaration: {
        type: "FunctionDeclaration",
        generator: true,
        async: true,
      },
    });
  });

  it("materializes keyword and private member names", () => {
    const source = [
      "AsyncGeneratorPrototype.return;",
      "class C { #field = 1; read(object) { return object?.#field; } }",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].expression.property).toMatchObject({
      type: "Identifier",
      name: "return",
    });
    const returned = result.program.body[1].body.body[1].value.body.body[0].argument;
    expect(returned).toMatchObject({
      type: "ChainExpression",
      expression: {
        type: "MemberExpression",
        optional: true,
        property: { type: "PrivateIdentifier", name: "field" },
      },
    });
  });

  it("materializes object and class generator methods", () => {
    const source = [
      "const methods = { *plain(value) { yield value; }, *[key]() {}, async *stream() { await load(); } };",
      "class Methods { static *values() {} static async *entries() {} }",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const [plain, computed, stream] = result.program.body[0].declarations[0].init.properties;
    expect(plain).toMatchObject({
      type: "Property",
      method: true,
      computed: false,
      value: { type: "FunctionExpression", generator: true, async: false },
    });
    expect(computed).toMatchObject({
      type: "Property",
      method: true,
      computed: true,
      key: { type: "Identifier", name: "key" },
      value: { type: "FunctionExpression", generator: true, async: false },
    });
    expect(stream.value).toMatchObject({
      type: "FunctionExpression",
      generator: true,
      async: true,
    });
    const [values, entries] = result.program.body[1].body.body;
    expect(values).toMatchObject({
      type: "MethodDefinition",
      static: true,
      computed: false,
      value: { type: "FunctionExpression", generator: true, async: false },
    });
    expect(entries.value).toMatchObject({
      type: "FunctionExpression",
      generator: true,
      async: true,
    });
  });

  it("materializes object and class async methods", () => {
    const source = [
      "const methods = { async plain(value) { await load(value); }, async [key]() {}, async 'named'() {}, async() {} };",
      "class Methods { async plain(value) { await load(value); } static async [key]() {} async #private() {} static async #privateStatic() {} async() {} }",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const objectMethods = result.program.body[0].declarations[0].init.properties;
    for (const method of objectMethods.slice(0, 3)) {
      expect(method).toMatchObject({
        type: "Property",
        method: true,
        value: { type: "FunctionExpression", async: true, generator: false },
      });
    }
    expect(source.slice(objectMethods[0].start, objectMethods[0].end)).toBe(
      "async plain(value) { await load(value); }",
    );
    expect(objectMethods[1]).toMatchObject({ computed: true, key: { name: "key" } });
    expect(objectMethods[2].key).toMatchObject({ type: "Literal", value: "named" });
    expect(objectMethods[3]).toMatchObject({
      key: { type: "Identifier", name: "async" },
      value: { async: false, generator: false },
    });

    const classMethods = result.program.body[1].body.body;
    for (const method of classMethods.slice(0, 4)) {
      expect(method).toMatchObject({
        type: "MethodDefinition",
        value: { type: "FunctionExpression", async: true, generator: false },
      });
    }
    expect(source.slice(classMethods[1].start, classMethods[1].end)).toBe(
      "static async [key]() {}",
    );
    expect(classMethods[1]).toMatchObject({ static: true, computed: true });
    expect(classMethods[2].key).toMatchObject({ type: "PrivateIdentifier", name: "private" });
    expect(classMethods[3]).toMatchObject({
      static: true,
      key: { type: "PrivateIdentifier", name: "privateStatic" },
    });
    expect(classMethods[4]).toMatchObject({
      key: { type: "Identifier", name: "async" },
      value: { async: false, generator: false },
    });
  });

  it("keeps escaped and line-broken async method introducers separate", () => {
    const clean = parse("class Methods { async\nplain() {} \\u0061sync() {} }", {
      semanticErrors: true,
    });
    expect(clean.diagnostics).toEqual([]);
    expect(clean.program.body[0].body.body).toMatchObject([
      { type: "PropertyDefinition", key: { name: "async" } },
      { type: "MethodDefinition", value: { async: false } },
      { type: "MethodDefinition", value: { async: false } },
    ]);

    for (
      const source of [
        "class Methods { \\u0061sync method() {} }",
        "const methods = { async\nmethod() {} };",
      ]
    ) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("diagnoses async method early errors", () => {
    for (
      const source of [
        "const methods = { async invalid(await) {} };",
        "class Methods { async invalid() { var \\u0061wait; } }",
        "class Methods { static async invalid() { void \\u0061wait; } }",
        "const methods = { async invalid(value = fallback) { 'use strict'; } };",
        "class Methods { async invalid(...values,) {} }",
        "const methods = { async invalid(value, value) {} };",
        "const methods = { async invalid() { super(); } };",
        "class Methods extends Base { async invalid() { return function() { return super.value; }; } }",
        "class Outer extends Base { constructor() { class Inner { static { super(); } } } }",
      ]
    ) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("materializes public class accessors and preserves get/set member ambiguities", () => {
    const source = [
      "class Accessors {",
      "  get value() { return this._value; }",
      "  set value({ next } = fallback) { this._value = next; }",
      "  static get [key]() { return value; }",
      "  static set 'named'(value) {}",
      "  get() {}",
      "  set(value) {}",
      "  get;",
      "  set = value;",
      "  get static() {}",
      "  static get static() {}",
      "}",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const members = result.program.body[0].body.body;
    expect(members).toMatchObject([
      {
        type: "MethodDefinition",
        kind: "get",
        key: { type: "Identifier", name: "value" },
        computed: false,
        static: false,
        value: { type: "FunctionExpression", params: [], generator: false, async: false },
      },
      {
        type: "MethodDefinition",
        kind: "set",
        key: { type: "Identifier", name: "value" },
        computed: false,
        static: false,
        value: {
          type: "FunctionExpression",
          params: [{ type: "AssignmentPattern", left: { type: "ObjectPattern" } }],
          generator: false,
          async: false,
        },
      },
      { type: "MethodDefinition", kind: "get", computed: true, static: true },
      {
        type: "MethodDefinition",
        kind: "set",
        key: { type: "Literal", value: "named" },
        computed: false,
        static: true,
      },
      { type: "MethodDefinition", kind: "method", key: { name: "get" } },
      { type: "MethodDefinition", kind: "method", key: { name: "set" } },
      { type: "PropertyDefinition", key: { name: "get" } },
      { type: "PropertyDefinition", key: { name: "set" } },
      { type: "MethodDefinition", kind: "get", key: { name: "static" }, static: false },
      { type: "MethodDefinition", kind: "get", key: { name: "static" }, static: true },
    ]);
  });

  it("materializes canonical private class accessors", () => {
    const source = [
      "class Accessors {",
      "  get #\\u0076alue() { return this.#value; }",
      "  set #value({ next } = fallback) { this.#value = next; }",
      "  static get #π() { return this.#π; }",
      "  static set #π(value) {}",
      "}",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const members = result.program.body[0].body.body;
    expect(members).toMatchObject([
      {
        type: "MethodDefinition",
        kind: "get",
        key: { type: "PrivateIdentifier", name: "value" },
        computed: false,
        static: false,
        value: { type: "FunctionExpression", params: [], generator: false, async: false },
      },
      {
        type: "MethodDefinition",
        kind: "set",
        key: { type: "PrivateIdentifier", name: "value" },
        computed: false,
        static: false,
        value: {
          type: "FunctionExpression",
          params: [{ type: "AssignmentPattern", left: { type: "ObjectPattern" } }],
          generator: false,
          async: false,
        },
      },
      {
        type: "MethodDefinition",
        kind: "get",
        key: { type: "PrivateIdentifier", name: "π" },
        computed: false,
        static: true,
      },
      {
        type: "MethodDefinition",
        kind: "set",
        key: { type: "PrivateIdentifier", name: "π" },
        computed: false,
        static: true,
      },
    ]);
    expect(members[0].value.body.body[0].argument.property).toMatchObject({
      type: "PrivateIdentifier",
      name: "value",
    });
    expect(source.slice(members[0].key.start, members[0].key.end)).toBe("#\\u0076alue");
  });

  it("diagnoses public class accessor early errors and unsupported introducers", () => {
    const sources = [
      "class C { get value(parameter) {} }",
      "class C { set value() {} }",
      "class C { set value(first, second) {} }",
      "class C { set value(...values) {} }",
      "class C { set value(parameter,) {} }",
      "class C { get constructor() {} }",
      "class C { set 'constructor'(value) {} }",
      "class C { get \"constr\\u0075ctor\"() {} }",
      "class C { set constr\\u0075ctor(value) {} }",
      "class C { static get prototype() {} }",
      "class C { static set 'prototype'(value) {} }",
      "class C { static get prot\\u006ftype() {} }",
      "class C extends Base { get value() { super(); } }",
      "class C extends Base { static set value(next) { super(); } }",
      "class C { get value() { with (object) statement; } }",
      "class C { set value(next) { delete identifier; } }",
      "class C { g\\u0065t value() {} }",
      "class C { s\\u0065t value(next) {} }",
    ];

    for (const source of sources) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }
  });

  it("diagnoses private class accessor early errors and collisions", () => {
    const sources = [
      "class C { get #value(parameter) {} }",
      "class C { set #value() {} }",
      "class C { set #value(...values) {} }",
      "class C { set #value(parameter,) {} }",
      "class C { get #value() {} get #value() {} }",
      "class C { set #value(next) {} set #value(next) {} }",
      "class C { #value; get #value() {} }",
      "class C { set #value(next) {} #value; }",
      "class C { #value() {} set #value(next) {} }",
      "class C { get #value() {} #value() {} }",
      "class C { get #value() {} static set #value(next) {} }",
      "class C { static get #\\u0076alue() {} set #value(next) {} }",
      "class C { get #constructor() {} }",
      "class C extends Base { get #value() { super(); } }",
      "class C { set #value(next) { with (object) statement; } }",
      "class C { g\\u0065t #value() {} }",
      "class C { get # value() {} }",
      "const object = { get #value() {} };",
      "class C { method() { return { set #value(next) {} }; } }",
    ];

    for (const source of sources) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }
  });

  it("materializes public object accessors and preserves get/set property ambiguities", () => {
    const source = [
      "const accessors = {",
      "  get value() { return this._value; },",
      "  set value({ next } = fallback) { this._value = next; },",
      "  get [key]() { return value; },",
      "  set 'named'(value) {},",
      "  get 0() {},",
      "  set 1n(value) {},",
      "  get return() {},",
      "  set async(value) {},",
      "  get() {},",
      "  set(value) {},",
      "  get,",
      "  set,",
      "  get: getter,",
      "  set: setter,",
      "  *get() {},",
      "  async *set() {},",
      "};",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const properties = result.program.body[0].declarations[0].init.properties;
    expect(properties.slice(0, 8)).toMatchObject([
      {
        type: "Property",
        kind: "get",
        key: { type: "Identifier", name: "value" },
        method: false,
        shorthand: false,
        computed: false,
        value: { type: "FunctionExpression", params: [], generator: false, async: false },
      },
      {
        type: "Property",
        kind: "set",
        key: { type: "Identifier", name: "value" },
        method: false,
        shorthand: false,
        computed: false,
        value: {
          type: "FunctionExpression",
          params: [{ type: "AssignmentPattern", left: { type: "ObjectPattern" } }],
          generator: false,
          async: false,
        },
      },
      { type: "Property", kind: "get", computed: true, method: false, shorthand: false },
      {
        type: "Property",
        kind: "set",
        key: { type: "Literal", value: "named" },
        computed: false,
      },
      { type: "Property", kind: "get", key: { type: "Literal", value: 0 } },
      { type: "Property", kind: "set", key: { type: "Literal", bigint: "1" } },
      { type: "Property", kind: "get", key: { type: "Identifier", name: "return" } },
      { type: "Property", kind: "set", key: { type: "Identifier", name: "async" } },
    ]);
    expect(properties.slice(8)).toMatchObject([
      { type: "Property", kind: "init", key: { name: "get" }, method: true },
      { type: "Property", kind: "init", key: { name: "set" }, method: true },
      { type: "Property", kind: "init", key: { name: "get" }, shorthand: true },
      { type: "Property", kind: "init", key: { name: "set" }, shorthand: true },
      { type: "Property", kind: "init", key: { name: "get" }, method: false },
      { type: "Property", kind: "init", key: { name: "set" }, method: false },
      {
        type: "Property",
        kind: "init",
        key: { name: "get" },
        method: true,
        value: { generator: true, async: false },
      },
      {
        type: "Property",
        kind: "init",
        key: { name: "set" },
        method: true,
        value: { generator: true, async: true },
      },
    ]);

    const strictNames = parse(
      [
        "const names = { static: 1, interface: 2, get public() { delete target.static; return target.static + target.interface; }, set private(value) { target.protected = value; } };",
        "class C { #key; get value() { delete target[this.#key]; return target[this.#key]; } }",
      ].join("\n"),
      { semanticErrors: true, sourceType: "module" },
    );
    expect(strictNames.diagnostics).toEqual([]);
  });

  it("diagnoses public object accessor early errors and unsupported introducers", () => {
    const sources = [
      "const object = { get value(parameter) {} };",
      "const object = { set value() {} };",
      "const object = { set value(first, second) {} };",
      "const object = { set value(...values) {} };",
      "const object = { set value(parameter,) {} };",
      "const object = { get value() { super(); } };",
      "const object = { set value(next) { super(); } };",
      "const object = { g\\u0065t value() {} };",
      "const object = { s\\u0065t value(next) {} };",
      "const object = { get *value() {} };",
      "0, [{ get value() {} }] = [{}];",
      "for ([{ set value(next) {} }] of source) {}",
      "[{ set value(next) {} }?.value = 1] = [2];",
      "const object = { get value() { 'use strict'; public = 1; } };",
      "const object = { set value(eval) { 'use strict'; } };",
      "const object = { set value(next = 0) { 'use strict'; } };",
      "class C { #value; method() { delete this.#value; } }",
    ];

    for (const source of sources) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }

    const moduleSources = [
      "const object = { get value() { export default null; } };",
      "const object = { set value(next) { import value from './value.js'; } };",
      "const object = { set value(eval) {} };",
    ];
    for (const source of moduleSources) {
      const result = parse(source, { semanticErrors: true, sourceType: "module" });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
    }

    const preservedPrivateDelete = parse(
      "class C { #value; method() { delete (this.#value); } }",
      { preserveParens: true, semanticErrors: true },
    );
    expect(preservedPrivateDelete.diagnostics).not.toEqual([]);

    const hashbangStrict = parse("#!/usr/bin/env node\n'use strict'; with (target) statement;", {
      semanticErrors: true,
    });
    expect(hashbangStrict.diagnostics).not.toEqual([]);
  });

  it("materializes property names across objects, patterns, and classes", () => {
    const source = [
      "const object = { [key]: value, return: keyword, 0: numeric, 1n: bigint, shorthand, [method]() {} };",
      "const { [key]: computed, return: renamed, 0: zero, 1n: big, shorthand = fallback } = source;",
      "({ [key]: target, return: other } = source);",
      "class Properties { [field] = value; [method]() {} return() {} 1n = value; }",
    ].join("\n");
    const result = parse(source, { semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    const properties = result.program.body[0].declarations[0].init.properties;
    expect(properties).toMatchObject([
      { computed: true, method: false, shorthand: false },
      { key: { type: "Identifier", name: "return" }, computed: false, shorthand: false },
      { key: { type: "Literal", value: 0 }, computed: false },
      { key: { type: "Literal", bigint: "1" }, computed: false },
      { key: { type: "Identifier", name: "shorthand" }, shorthand: true },
      {
        computed: true,
        method: true,
        value: { type: "FunctionExpression", generator: false, async: false },
      },
    ]);
    const binding = result.program.body[1].declarations[0].id;
    expect(binding.properties).toMatchObject([
      { computed: true, shorthand: false },
      { key: { name: "return" }, computed: false, shorthand: false },
      { key: { value: 0 }, computed: false },
      { key: { bigint: "1" }, computed: false },
      { shorthand: true, value: { type: "AssignmentPattern" } },
    ]);
    const assignment = result.program.body[2].expression.expression.left;
    expect(assignment).toMatchObject({
      type: "ObjectPattern",
      properties: [{ computed: true }, { key: { name: "return" } }],
    });
    expect(result.program.body[3].body.body).toMatchObject([
      { type: "PropertyDefinition", computed: true },
      { type: "MethodDefinition", computed: true },
      { type: "MethodDefinition", key: { name: "return" }, computed: false },
      { type: "PropertyDefinition", key: { bigint: "1" }, computed: false },
    ]);
  });

  it("separates declaration initializers from binding defaults", () => {
    const source = [
      "const value = source, second = other;",
      "for (let index = 0; index < limit; index++) {}",
      "function defaults(value = fallback, { key } = object, [first] = list, ...rest) {}",
      "const [nested = 1, { item: renamed = fallback }, [inner] = list] = source;",
    ].join("\n");
    const result = parse(source, { sourceType: "script" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].declarations).toMatchObject([
      {
        id: { type: "Identifier", name: "value" },
        init: { type: "Identifier", name: "source" },
      },
      {
        id: { type: "Identifier", name: "second" },
        init: { type: "Identifier", name: "other" },
      },
    ]);
    expect(result.program.body[1].init.declarations[0]).toMatchObject({
      id: { type: "Identifier", name: "index" },
      init: { type: "Literal", value: 0 },
    });
    expect(result.program.body[2].params).toMatchObject([
      { type: "AssignmentPattern", left: { type: "Identifier", name: "value" } },
      { type: "AssignmentPattern", left: { type: "ObjectPattern" } },
      { type: "AssignmentPattern", left: { type: "ArrayPattern" } },
      { type: "RestElement", argument: { type: "Identifier", name: "rest" } },
    ]);
    expect(result.program.body[3].declarations[0]).toMatchObject({
      id: {
        type: "ArrayPattern",
        elements: [
          { type: "AssignmentPattern" },
          {
            type: "ObjectPattern",
            properties: [{ value: { type: "AssignmentPattern" } }],
          },
          { type: "AssignmentPattern", left: { type: "ArrayPattern" } },
        ],
      },
      init: { type: "Identifier", name: "source" },
    });
  });

  it("diagnoses defaults on rest bindings without panicking", () => {
    const sources = [
      "function invalid(...rest = fallback) {}",
      "const [...rest = fallback] = source;",
      "const { ...rest = fallback } = source;",
    ];

    for (const source of sources) {
      const result = parse(source, { sourceType: "script" });
      expect(result.diagnostics).not.toEqual([]);
      expect(result.panicked).toBe(false);
      expect(result.program.type).toBe("Program");
    }
  });

  it("materializes catch binding patterns in an isolated scope", () => {
    const source = [
      "let message;",
      "try {} catch ({ message, code = 1, ...rest }) {}",
      "try {} catch ([first, , third = 3, ...tail]) {}",
      "try {} catch {}",
    ].join("\n");
    const result = parse(source, { semanticErrors: true, sourceType: "script" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[1].handler.param).toMatchObject({
      type: "ObjectPattern",
      properties: [
        {
          type: "Property",
          key: { type: "Identifier", name: "message" },
          value: { type: "Identifier", name: "message" },
          shorthand: true,
        },
        {
          type: "Property",
          key: { type: "Identifier", name: "code" },
          value: {
            type: "AssignmentPattern",
            left: { type: "Identifier", name: "code" },
            right: { type: "Literal", value: 1 },
          },
          shorthand: true,
        },
        { type: "RestElement", argument: { type: "Identifier", name: "rest" } },
      ],
    });
    expect(result.program.body[2].handler.param).toMatchObject({
      type: "ArrayPattern",
      elements: [
        { type: "Identifier", name: "first" },
        null,
        {
          type: "AssignmentPattern",
          left: { type: "Identifier", name: "third" },
          right: { type: "Literal", value: 3 },
        },
        { type: "RestElement", argument: { type: "Identifier", name: "tail" } },
      ],
    });
    expect(result.program.body[3].handler.param).toBeNull();
  });

  it("diagnoses a default on the catch parameter", () => {
    const result = parse("try {} catch (error = fallback) {}", { sourceType: "script" });

    expect(result.diagnostics[0]).toBe("expected RightParen, found Eq");
    expect(result.panicked).toBe(false);
    expect(result.program.type).toBe("Program");
  });

  it("diagnoses commas following catch rest bindings", () => {
    const cases = [
      ["try {} catch ([...rest, tail]) {}", "rest element must be last"],
      ["try {} catch ([...rest,]) {}", "rest element must be last"],
      ["try {} catch ({ ...rest, }) {}", "rest property must be last"],
    ];

    for (const [source, expected] of cases) {
      const result = parse(source, { sourceType: "script" });
      expect(result.diagnostics).toEqual([expected]);
      expect(result.panicked).toBe(false);
      expect(result.program.type).toBe("Program");
    }
  });

  it("exposes assignment-target validation through the Node API", () => {
    const valid = parse("await = 1; yield = 2; ({ value } = source); factory() += 1;", {
      semanticErrors: true,
      sourceType: "script",
    });
    expect(valid.diagnostics).toEqual([]);
    expect(valid.program.body[2].expression.expression.left.type).toBe("ObjectPattern");

    const constructedMember = parse("new Constructor().member = value;", {
      semanticErrors: true,
      sourceType: "script",
    });
    expect(constructedMember.diagnostics).toEqual([]);
    expect(constructedMember.program.body[0].expression.left.type).toBe("MemberExpression");
    expect(constructedMember.program.body[0].expression.left.object.type).toBe("NewExpression");

    for (
      const source of [
        "factory() ||= source;",
        "target?.member++;",
        "({ value }) = source;",
        "'use strict'; (eval) = source;",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked).toBe(false);
      expect(result.program.type).toBe("Program");
    }

    const optionalAssignment = parse("target?.member = value; target?.member &&= value;", {
      optionalChainingAssign: true,
      semanticErrors: true,
      sourceType: "script",
    });
    expect(optionalAssignment.diagnostics).toEqual([]);

    const typescriptJs = parse("function load() { await new Promise(undefined); }", {
      semanticErrors: true,
      sourceType: "script",
      typescriptJsCompatibility: true,
    });
    expect(typescriptJs.diagnostics).toEqual([]);
  });
});
