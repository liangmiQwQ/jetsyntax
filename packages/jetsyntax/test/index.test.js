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

  it("accepts dynamic import trailing commas without inventing options", () => {
    const result = parse(
      [
        "const sourceOnly = import('source',);",
        "const withOptions = import('data.json', { with: { type: 'json' } },);",
      ].join("\n"),
      { sourceType: "module" },
    );

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        declarations: [
          { init: { type: "ImportExpression", source: { value: "source" }, options: null } },
        ],
      },
      {
        declarations: [
          {
            init: {
              type: "ImportExpression",
              source: { value: "data.json" },
              options: { type: "ObjectExpression" },
            },
          },
        ],
      },
    ]);

    for (
      const source of [
        "import(,);",
        "import('source',,);",
        "import('source', {}, extra);",
        "import('source', {},,);",
      ]
    ) {
      expect(parse(source, { sourceType: "script" }).diagnostics, source).not.toEqual([]);
    }
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

  it("isolates switch case bindings and diagnoses invalid case blocks", () => {
    const allowed = parse(
      "let outer; let shared; switch (0) { case 0: let outer; function same() {} "
        + "default: function same() {} } "
        + "switch (1) { case 1: let local; } switch (2) { default: let local; } "
        + "function make() { var C = class C extends C {}; } "
        + "class Static { static { var Static; var shared; var local; function local() {} "
        + "function reversed() {} var reversed; } }",
      { semanticErrors: true, sourceType: "script" },
    );
    expect(allowed.diagnostics).toEqual([]);

    for (
      const source of [
        "switch (0) { case 0: let value; default: const value = 1; }",
        "switch (0) { case 0: function value() {} default: var value; }",
        "\"use strict\"; switch (0) { case 0: function value() {} "
        + "default: function value() {} }",
        "switch (value) { default: first; default: second; }",
        "class C { static { var value; let value; } }",
        "class C { static { let value; var value; } }",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }

    expect(
      parse(
        "let leaked; outer: while (value) { class C { "
          + "static { var C; var leaked; break outer; } } } "
          + "switch (value) { default: first; default: second; }",
        { semanticErrors: false },
      ).diagnostics,
    ).toEqual([]);
  });

  it("materializes invalid template escapes by tagged context", () => {
    const taggedSource =
      "tag`\\01`; tag`\\xg`; tag`\\u0`; tag`\\u{}`; tag`\\u{110000}`; tag`\\xg${value}\\u0${other}\\u{g}`;";
    const tagged = parse(taggedSource, { range: true });

    expect(tagged.diagnostics).toEqual([]);
    const quasis = tagged.program.body.flatMap(statement => statement.expression.quasi.quasis);
    expect(quasis).toHaveLength(8);
    expect(quasis.every(quasi => quasi.value.cooked === null)).toBe(true);
    expect(quasis.map(quasi => quasi.value.raw)).toEqual([
      "\\01",
      "\\xg",
      "\\u0",
      "\\u{}",
      "\\u{110000}",
      "\\xg",
      "\\u0",
      "\\u{g}",
    ]);
    expect(quasis.map(quasi => quasi.tail)).toEqual([
      true,
      true,
      true,
      true,
      true,
      false,
      false,
      true,
    ]);
    for (const quasi of quasis) {
      expect(quasi.range).toEqual([quasi.start, quasi.end]);
      expect(taggedSource.slice(quasi.start, quasi.end)).toBe(quasi.value.raw);
    }

    const untaggedSource = "`\\01`; `\\xg`; `\\u0`; `\\u{}`; `\\u{110000}`;";
    const untagged = parse(untaggedSource, { range: true });
    expect(untagged.diagnostics).toEqual(
      Array(5).fill("invalid escape sequence in template literal"),
    );
    expect(
      untagged.program.body.every(
        statement => statement.expression.quasis[0].value.cooked === null,
      ),
    ).toBe(true);
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

  it("materializes anonymous default export declarations with null identifiers", () => {
    const cases = [
      ["export default function() {}", "FunctionDeclaration", false, false],
      ["export default function*() { yield value; }", "FunctionDeclaration", true, false],
      ["export default async function() { await value; }", "FunctionDeclaration", false, true],
      ["export default async function*() { yield await value; }", "FunctionDeclaration", true, true],
      ["export default class {}", "ClassDeclaration"],
    ];

    for (const [source, type, generator, asynchronous] of cases) {
      const result = parse(source, { semanticErrors: true, sourceType: "module" });

      expect(result.diagnostics, source).toEqual([]);
      expect(result.program.body[0]).toMatchObject({
        type: "ExportDefaultDeclaration",
        declaration: {
          type,
          id: null,
          ...(type === "FunctionDeclaration" ? { generator, async: asynchronous } : {}),
        },
      });
    }

    for (const source of ["export default function<T>() {}", "export default class<T> {}"]) {
      const result = parse(source, { lang: "ts", semanticErrors: true, sourceType: "module" });
      expect(result.diagnostics, source).toEqual([]);
      expect(result.program.body[0].declaration.id).toBeNull();
    }

    for (
      const source of ["function() {}", "class {}", "export function() {}", "export class {}"]
    ) {
      expect(parse(source, { semanticErrors: true, sourceType: "module" }).diagnostics, source).not
        .toEqual([]);
    }

    const named = parse(
      "export default function named() {}",
      { semanticErrors: true, sourceType: "module" },
    );
    expect(named.diagnostics).toEqual([]);
    expect(named.program.body[0].declaration.id).toMatchObject({ name: "named" });
  });

  it("materializes string-named module imports and exports", () => {
    const source = [
      "import { \"default\" as value } from \"source\";",
      "export { \"source name\", \"source name\" as \"public name\" } from \"source\";",
      "export { value as \"local name\" };",
      "export * as \"namespace name\" from \"source\";",
    ].join("\n");
    const result = parse(source, { semanticErrors: true, sourceType: "module" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "ImportDeclaration",
        specifiers: [
          {
            type: "ImportSpecifier",
            imported: { type: "Literal", value: "default", raw: "\"default\"" },
            local: { type: "Identifier", name: "value" },
          },
        ],
      },
      {
        type: "ExportNamedDeclaration",
        specifiers: [
          {
            type: "ExportSpecifier",
            local: { type: "Literal", value: "source name" },
            exported: { type: "Literal", value: "source name" },
          },
          {
            type: "ExportSpecifier",
            local: { type: "Literal", value: "source name" },
            exported: { type: "Literal", value: "public name" },
          },
        ],
      },
      {
        type: "ExportNamedDeclaration",
        specifiers: [
          {
            local: { type: "Identifier", name: "value" },
            exported: { type: "Literal", value: "local name" },
          },
        ],
      },
      {
        type: "ExportAllDeclaration",
        exported: { type: "Literal", value: "namespace name" },
      },
    ]);
  });

  it("preserves TypeScript type-only kinds around string module names", () => {
    const source = [
      "import type { \"source\" as Imported } from \"module\";",
      "import { type \"source\" as Specifier } from \"module\";",
      "export type { Imported as \"public\" };",
      "export type * as \"namespace\" from \"module\";",
    ].join("\n");
    const result = parse(source, { lang: "ts", semanticErrors: true, sourceType: "module" });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      {
        type: "ImportDeclaration",
        importKind: "type",
        specifiers: [{ imported: { value: "source" }, importKind: "value" }],
      },
      {
        type: "ImportDeclaration",
        importKind: "value",
        specifiers: [{ imported: { value: "source" }, importKind: "type" }],
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        specifiers: [{ exportKind: "value" }],
      },
      { type: "ExportAllDeclaration", exportKind: "type", exported: { value: "namespace" } },
    ]);
  });

  it("disambiguates contextual type-only import specifiers", () => {
    const specifiers = [
      "import { type } from 'module';",
      "import { type as } from 'module';",
      "import { type as as } from 'module';",
      "import { type as as as } from 'module';",
    ].map((source) => {
      const result = parse(source, { lang: "ts", semanticErrors: true, sourceType: "module" });
      expect(result.diagnostics).toEqual([]);
      return result.program.body[0].specifiers[0];
    });

    expect(specifiers).toMatchObject([
      { imported: { name: "type" }, local: { name: "type" }, importKind: "value" },
      { imported: { name: "as" }, local: { name: "as" }, importKind: "type" },
      { imported: { name: "type" }, local: { name: "as" }, importKind: "value" },
      { imported: { name: "as" }, local: { name: "as" }, importKind: "type" },
    ]);
  });

  it("preserves type-only specifier ranges and deferred import phases", () => {
    const typed = parse(
      "import { type Value } from 'module'; export { type Value as Public } from 'module';",
      { lang: "ts", range: true, semanticErrors: true, sourceType: "module" },
    );

    expect(typed.diagnostics).toEqual([]);
    expect(typed.program.body[0].specifiers[0]).toMatchObject({
      importKind: "type",
      range: [9, 19],
    });
    expect(typed.program.body[1].specifiers[0]).toMatchObject({
      exportKind: "type",
      range: [46, 66],
    });

    const deferred = parse("import defer * as namespace from 'module';", {
      semanticErrors: true,
      sourceType: "module",
    });
    expect(deferred.diagnostics).toEqual([]);
    expect(deferred.program.body[0]).toMatchObject({
      type: "ImportDeclaration",
      phase: "defer",
      specifiers: [{ type: "ImportNamespaceSpecifier", local: { name: "namespace" } }],
    });

    const invalidDeferred = parse("import defer { named } from 'module';", {
      lang: "ts",
      semanticErrors: true,
      sourceType: "module",
    });
    expect(invalidDeferred.diagnostics).not.toEqual([]);
    expect(invalidDeferred.program.body[0]).toMatchObject({
      type: "ImportDeclaration",
      phase: "defer",
      specifiers: [{ type: "ImportSpecifier", local: { name: "named" } }],
    });

    const invalidDefault = parse("import defer local from 'module';", {
      lang: "ts",
      semanticErrors: true,
      sourceType: "module",
    });
    expect(invalidDefault.diagnostics).not.toEqual([]);
    expect(invalidDefault.program.body).toMatchObject([{
      type: "ImportDeclaration",
      phase: "defer",
      specifiers: [{ type: "ImportDefaultSpecifier", local: { name: "local" } }],
      source: { value: "module" },
    }]);

    const invalidCombined = parse("import defer local, * as namespace from 'module';", {
      semanticErrors: true,
      sourceType: "module",
    });
    expect(invalidCombined.diagnostics).not.toEqual([]);
    expect(invalidCombined.program.body[0]).toMatchObject({
      type: "ImportDeclaration",
      phase: "defer",
      specifiers: [
        { type: "ImportDefaultSpecifier", local: { name: "local" } },
        { type: "ImportNamespaceSpecifier", local: { name: "namespace" } },
      ],
    });
  });

  it("diagnoses repeated TypeScript import bindings", () => {
    for (
      const source of [
        "import { Named } from 'one'; import { Named } from 'two';",
        "import type { Named } from 'one'; import type { Named } from 'two';",
        "import { type Named } from 'one'; import { type Named } from 'two';",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true, sourceType: "module" });
      expect(result.diagnostics, source).not.toEqual([]);
    }
  });

  it("tracks merged variable names in export declarations", () => {
    const result = parse("var name; export var name; export { name };", {
      semanticErrors: true,
      sourceType: "module",
    });
    expect(result.diagnostics).toContain("duplicate export `name`");
  });

  it("ends bare yield expressions at enclosing expression boundaries", () => {
    const source = [
      "function* sequence(value) {",
      "  const array = [yield, yield yield];",
      "  const object = { key: yield, ...yield };",
      "  consume(yield);",
      "  switch (value) { case yield: break; }",
      "}",
    ].join("\n");
    const result = parse(source, { semanticErrors: true, sourceType: "script" });

    expect(result.diagnostics).toEqual([]);
    const declaration = result.program.body[0];
    const yields = [];
    const visit = (value) => {
      if (!value || typeof value !== "object") return;
      if (value.type === "YieldExpression") yields.push(value);
      for (const child of Object.values(value)) {
        if (Array.isArray(child)) child.forEach(visit);
        else visit(child);
      }
    };
    visit(declaration);
    expect(yields).toHaveLength(7);
    expect(yields.filter(expression => expression.argument === null)).toHaveLength(6);
    expect(yields.every(expression => expression.delegate === false)).toBe(true);

    for (
      const invalid of [
        "function* sequence() { yield ? one : two; }",
        "function* sequence(source) { yield in source; }",
        "function* sequence() { (value = yield) => value; }",
      ]
    ) {
      expect(parse(invalid, { semanticErrors: true }).diagnostics, invalid).not.toEqual([]);
    }

    const nested = parse(
      "function* outer() { (value = function* () { yield; }) => value; }",
      { semanticErrors: true },
    );
    expect(nested.diagnostics).toEqual([]);
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

  it("materializes entity-name TypeScript type queries", () => {
    // The line break in Boundary leaves `<T>` for the following call signature.
    const source = [
      "type Plain = typeof value;",
      "type Qualified = typeof Namespace.value;",
      "type Current = typeof this;",
      "type Generic = typeof factory<Input>;",
      "interface Boundary {",
      "  (value: Input): typeof value",
      "  <T>(): void",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [plain, qualified, current, generic, boundary] = result.program.body;
    const queries = [plain, qualified, current, generic].map(alias => alias.typeAnnotation);
    expect(queries).toMatchObject([
      {
        type: "TSTypeQuery",
        exprName: { type: "Identifier", name: "value" },
      },
      {
        type: "TSTypeQuery",
        exprName: {
          type: "TSQualifiedName",
          left: { type: "Identifier", name: "Namespace" },
          right: { type: "Identifier", name: "value" },
        },
      },
      {
        type: "TSTypeQuery",
        exprName: { type: "ThisExpression" },
      },
      {
        type: "TSTypeQuery",
        exprName: { type: "Identifier", name: "factory" },
        typeArguments: {
          type: "TSTypeParameterInstantiation",
          params: [{ type: "TSTypeReference", typeName: { name: "Input" } }],
        },
      },
    ]);
    expect(queries.slice(0, 3).every(query => !("typeArguments" in query))).toBe(true);
    expect(queries.map(query => source.slice(query.start, query.end))).toEqual([
      "typeof value",
      "typeof Namespace.value",
      "typeof this",
      "typeof factory<Input>",
    ]);
    for (const query of queries) {
      expect(query.range).toEqual([query.start, query.end]);
      expect(query.exprName.range).toEqual([query.exprName.start, query.exprName.end]);
    }
    expect(source.slice(generic.typeAnnotation.typeArguments.start, generic.typeAnnotation.typeArguments.end))
      .toBe("<Input>");
    expect(generic.typeAnnotation.typeArguments.range).toEqual([
      generic.typeAnnotation.typeArguments.start,
      generic.typeAnnotation.typeArguments.end,
    ]);

    const [querySignature, genericSignature] = boundary.body.body;
    expect([querySignature, genericSignature]).toMatchObject([
      {
        type: "TSCallSignatureDeclaration",
        returnType: {
          typeAnnotation: {
            type: "TSTypeQuery",
            exprName: { name: "value" },
          },
        },
      },
      {
        type: "TSCallSignatureDeclaration",
        typeParameters: {
          params: [{ name: { name: "T" } }],
        },
        returnType: { typeAnnotation: { type: "TSVoidKeyword" } },
      },
    ]);
    const boundaryQuery = querySignature.returnType.typeAnnotation;
    expect(boundaryQuery).not.toHaveProperty("typeArguments");
    expect(source.slice(boundaryQuery.start, boundaryQuery.end)).toBe("typeof value");
    expect(boundaryQuery.range).toEqual([boundaryQuery.start, boundaryQuery.end]);
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

  it("materializes TypeScript call and construct signatures", () => {
    const source = [
      "interface Callable {",
      "  <T>(value: T): T;",
      "  new (value: number): Service",
      "}",
      "type Literal = { (): void, new (): Service }",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].body.body).toMatchObject([
      {
        type: "TSCallSignatureDeclaration",
        typeParameters: {
          type: "TSTypeParameterDeclaration",
          params: [{ name: { name: "T" } }],
        },
        params: [
          {
            type: "Identifier",
            name: "value",
            typeAnnotation: { typeAnnotation: { type: "TSTypeReference" } },
          },
        ],
        returnType: { typeAnnotation: { type: "TSTypeReference" } },
      },
      {
        type: "TSConstructSignatureDeclaration",
        typeParameters: null,
        params: [
          {
            type: "Identifier",
            name: "value",
            typeAnnotation: { typeAnnotation: { type: "TSNumberKeyword" } },
          },
        ],
        returnType: { typeAnnotation: { type: "TSTypeReference" } },
      },
    ]);
    expect(result.program.body[1].typeAnnotation.members).toMatchObject([
      {
        type: "TSCallSignatureDeclaration",
        typeParameters: null,
        params: [],
        returnType: { typeAnnotation: { type: "TSVoidKeyword" } },
      },
      {
        type: "TSConstructSignatureDeclaration",
        typeParameters: null,
        params: [],
        returnType: { typeAnnotation: { type: "TSTypeReference" } },
      },
    ]);
    for (
      const signature of [
        ...result.program.body[0].body.body,
        ...result.program.body[1].typeAnnotation.members,
      ]
    ) {
      expect(signature.range).toEqual([signature.start, signature.end]);
      expect(source.slice(signature.start, signature.end)).not.toMatch(/[;,]$/u);
    }
  });

  it("materializes interface and type-literal index signatures", () => {
    const source = [
      "interface Dictionary {",
      "  [key: string]: number;",
      "  readonly [index: number]: string,",
      "  [symbol: symbol]",
      "  [yield: string]: boolean",
      "}",
      "type Lookup = { [name: string]: unknown }",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const signatures = [
      ...result.program.body[0].body.body,
      ...result.program.body[1].typeAnnotation.members,
    ];
    expect(signatures).toMatchObject([
      {
        type: "TSIndexSignature",
        parameters: [{
          type: "Identifier",
          name: "key",
          optional: false,
          typeAnnotation: { typeAnnotation: { type: "TSStringKeyword" } },
        }],
        typeAnnotation: { typeAnnotation: { type: "TSNumberKeyword" } },
        readonly: false,
        static: false,
      },
      {
        type: "TSIndexSignature",
        parameters: [{
          name: "index",
          typeAnnotation: { typeAnnotation: { type: "TSNumberKeyword" } },
        }],
        typeAnnotation: { typeAnnotation: { type: "TSStringKeyword" } },
        readonly: true,
      },
      {
        type: "TSIndexSignature",
        parameters: [{
          name: "symbol",
          typeAnnotation: { typeAnnotation: { type: "TSSymbolKeyword" } },
        }],
        typeAnnotation: null,
      },
      {
        type: "TSIndexSignature",
        parameters: [{
          name: "yield",
          typeAnnotation: { typeAnnotation: { type: "TSStringKeyword" } },
        }],
        typeAnnotation: { typeAnnotation: { type: "TSBooleanKeyword" } },
      },
      {
        type: "TSIndexSignature",
        parameters: [{
          name: "name",
          typeAnnotation: { typeAnnotation: { type: "TSStringKeyword" } },
        }],
        typeAnnotation: { typeAnnotation: { type: "TSUnknownKeyword" } },
      },
    ]);
    expect(signatures.map(signature => source.slice(signature.start, signature.end))).toEqual([
      "[key: string]: number",
      "readonly [index: number]: string",
      "[symbol: symbol]",
      "[yield: string]: boolean",
      "[name: string]: unknown",
    ]);
    for (const signature of signatures) {
      expect(signature.range).toEqual([signature.start, signature.end]);
    }

    for (
      const reserved of [
        "async function f() { interface I { [await: string]: number } }",
        "function* f() { interface I { [yield: string]: number } }",
      ]
    ) {
      const recovered = parse(reserved, { lang: "ts" });
      expect(recovered.diagnostics, reserved).not.toEqual([]);
      expect(JSON.stringify(recovered.program), reserved).not.toContain("TSIndexSignature");
    }
    for (
      const contextual of [
        "async function f() { type I = { [await: string]: number } }",
        "function* f() { type I = { [yield: string]: number } }",
      ]
    ) {
      expect(parse(contextual, { lang: "ts" }).diagnostics, contextual).toEqual([]);
    }
  });

  it("recovers noncanonical index parameters by semantic mode", () => {
    const source = [
      "interface Recovered {",
      "  [key: string,]: string;",
      "  [...rest]: string;",
      "  [public named: string]: number;",
      "  [optional?: number]: unknown;",
      "  [first: string, second: number]: boolean;",
      "  []: never;",
      "  [typedDefault: string = '']: number;",
      "  [...restDefault = 1]: string;",
      "  [public untyped]: number;",
      "}",
      "type Literal = { [untyped, other]: string }",
    ].join("\n");
    const syntax = parse(source, { lang: "ts" });

    expect(syntax.diagnostics).toEqual([]);
    const signatures = [
      ...syntax.program.body[0].body.body,
      ...syntax.program.body[1].typeAnnotation.members,
    ];
    expect(signatures.map(signature => signature.parameters.length)).toEqual([1, 1, 1, 1, 2, 0, 1, 1, 1, 2]);
    expect(signatures[1].parameters[0]).toMatchObject({
      type: "RestElement",
      argument: { type: "Identifier", name: "rest" },
    });
    expect(signatures[3].parameters[0]).toMatchObject({
      type: "Identifier",
      name: "optional",
      optional: true,
      typeAnnotation: { typeAnnotation: { type: "TSNumberKeyword" } },
    });
    expect(signatures[6].parameters[0]).toMatchObject({
      type: "AssignmentPattern",
      left: { type: "Identifier", name: "typedDefault" },
      right: { type: "Literal", value: "" },
    });
    expect(signatures[7].parameters[0]).toMatchObject({
      type: "RestElement",
      argument: {
        type: "AssignmentPattern",
        left: { type: "Identifier", name: "restDefault" },
        right: { type: "Literal", value: 1 },
      },
    });
    expect(signatures[8].parameters[0]).toMatchObject({ type: "Identifier", name: "untyped" });

    const semantic = parse(source, { lang: "ts", semanticErrors: true });
    expect(semantic.diagnostics).toEqual(expect.arrayContaining([
      "an index signature parameter cannot have a trailing comma",
      "an index signature parameter cannot be a rest parameter",
      "index signatures cannot have an accessibility modifier",
      "an index signature parameter cannot be optional",
      "an index signature parameter requires a type annotation",
      "an index signature parameter cannot have an initializer",
      "an index signature must have exactly one parameter",
    ]));
    expect(JSON.stringify(semantic.program).match(/TSIndexSignature/gu)).toHaveLength(10);

    for (const computed of ["[plain]", "[assigned = 0]", "[x ? y : z]"]) {
      const result = parse(`type Computed = { ${computed}: number }`, { lang: "ts" });
      expect(JSON.stringify(result.program), computed).not.toContain("TSIndexSignature");
    }
  });

  it("materializes TypeScript class index signatures and modifier recovery", () => {
    const source = [
      "class Dictionary {",
      "  [key: string]: number;",
      "  readonly [index: number]: string;",
      "  static\n  [name: string]: unknown;",
      "  static readonly [symbol: symbol]: boolean,",
      "}",
      "declare namespace N { class Ambient { [key: string]: number } }",
      "class Generic<T> { [key: string]: T }",
      "const Expression = class { [key: string]: unknown };",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const signatures = [
      ...result.program.body[0].body.body,
      ...result.program.body[1].body.body[0].body.body,
      ...result.program.body[2].body.body,
      ...result.program.body[3].declarations[0].init.body.body,
    ];
    expect(signatures).toMatchObject([
      { type: "TSIndexSignature", readonly: false, static: false },
      { type: "TSIndexSignature", readonly: true, static: false },
      { type: "TSIndexSignature", readonly: false, static: true },
      { type: "TSIndexSignature", readonly: true, static: true },
      { type: "TSIndexSignature", readonly: false, static: false },
      { type: "TSIndexSignature", readonly: false, static: false },
      { type: "TSIndexSignature", readonly: false, static: false },
    ]);
    expect(signatures.map(signature => source.slice(signature.start, signature.end))).toEqual([
      "[key: string]: number;",
      "readonly [index: number]: string;",
      "static\n  [name: string]: unknown;",
      "static readonly [symbol: symbol]: boolean,",
      "[key: string]: number",
      "[key: string]: T",
      "[key: string]: unknown",
    ]);
    for (const signature of signatures) expect(signature.range).toEqual([signature.start, signature.end]);

    const invalid = [
      "class Invalid extends Base {",
      "  readonly static [order: string]: number;",
      "  abstract [abstracted: string]: number;",
      "  declare [declared: string]: number;",
      "  private [privateKey: string]: number;",
      "  override [overridden: string]: number;",
      "  export [exported: string]: number;",
      "  declare readonly [declaredReadonly: string]: number;",
      "  export static readonly [exportedStatic: string]: number;",
      "  export\n  [exportedLine: string]: number;",
      "}",
    ].join("\n");
    const syntax = parse(invalid, { lang: "ts" });
    expect(syntax.diagnostics).toEqual([]);
    expect(syntax.program.body[0].body.body).toMatchObject([
      { type: "TSIndexSignature", readonly: true, static: true },
      { type: "TSIndexSignature", abstract: true },
      { type: "TSIndexSignature", declare: true },
      { type: "TSIndexSignature", accessibility: "private" },
      { type: "TSIndexSignature", override: true },
      { type: "TSIndexSignature", export: true },
      { type: "TSIndexSignature", declare: true, readonly: true },
      { type: "TSIndexSignature", export: true, static: true, readonly: true },
      { type: "TSIndexSignature", export: true },
    ]);
    const semantic = parse(invalid, { lang: "ts", semanticErrors: true });
    expect(semantic.diagnostics).toEqual(expect.arrayContaining([
      "TypeScript class member modifiers are out of order",
      "class index signatures cannot have the abstract modifier",
      "class index signatures cannot have the declare modifier",
      "class index signatures cannot have an accessibility modifier",
      "class index signatures cannot have the override modifier",
      "class index signatures cannot have the export modifier",
    ]));

    const boundaries = parse(
      "class C { declare\n[plain: string]: number; declare r\\u0065adonly [escaped: string]: number; }",
      { lang: "ts" },
    );
    expect(boundaries.diagnostics).toEqual([]);
    expect(boundaries.program.body[0].body.body).toMatchObject([
      { type: "PropertyDefinition", key: { name: "declare" } },
      { type: "TSIndexSignature", readonly: false },
      { type: "TSIndexSignature", declare: true, readonly: true },
    ]);

    const ambiguous = parse(
      "class Computed { [plain]: number; [assigned = 0]: number; [x ? y : z]: number; readonly\n[line: string]: number; readonly [computed]: number; static [alsoComputed]: number }",
      { lang: "ts" },
    );
    expect(ambiguous.diagnostics).toEqual([]);
    expect(JSON.stringify(ambiguous.program).match(/TSIndexSignature/gu)).toHaveLength(1);

    for (
      const reserved of [
        "async function f() { class C { [await: string]: number } }",
        "function* f() { class C { [yield: string]: number } }",
      ]
    ) {
      const recovered = parse(reserved, { lang: "ts" });
      expect(recovered.diagnostics, reserved).not.toEqual([]);
      expect(JSON.stringify(recovered.program), reserved).not.toContain("TSIndexSignature");
    }

    const javascript = parse("class C { [key: string]: number }");
    expect(javascript.diagnostics).not.toEqual([]);
    expect(JSON.stringify(javascript.program)).not.toContain("TSIndexSignature");

    const compatibility = parse("class C { [key: string]: number }", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0].body.body[0].type).toBe("TSIndexSignature");
  });

  it("materializes untyped TypeScript signature parameters", () => {
    const source = [
      "type Callback = (this, value, optional?) => void;",
      "interface Callable {",
      "  method(value): void;",
      "  (value, ...rest): boolean;",
      "  new (value, optional?): Callable;",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts" });

    expect(result.diagnostics).toEqual([]);
    const members = result.program.body[1].body.body;
    const parameterLists = [
      result.program.body[0].typeAnnotation.params,
      ...members.map(member => member.params),
    ];
    expect(parameterLists).toMatchObject([
      [
        { type: "Identifier", name: "this" },
        { type: "Identifier", name: "value" },
        { type: "Identifier", name: "optional", typeAnnotation: null, optional: true },
      ],
      [{ type: "Identifier", name: "value" }],
      [
        { type: "Identifier", name: "value" },
        { type: "RestElement", argument: { type: "Identifier", name: "rest" } },
      ],
      [
        { type: "Identifier", name: "value" },
        { type: "Identifier", name: "optional", typeAnnotation: null, optional: true },
      ],
    ]);
    for (const parameters of parameterLists) {
      for (const parameter of parameters) {
        const identifier = parameter.type === "RestElement" ? parameter.argument : parameter;
        if (["this", "value", "rest"].includes(identifier.name)) {
          expect(identifier).not.toHaveProperty("typeAnnotation");
          expect(identifier).not.toHaveProperty("optional");
        }
      }
    }
    for (
      const invalid of [
        "type Callback = (this?) => void;",
        "interface Callable { (...this): void }",
      ]
    ) {
      expect(parse(invalid, { lang: "ts" }).diagnostics, invalid).not.toEqual([]);
    }
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

  it("materializes TypeScript generic postfix expressions", () => {
    const source = [
      "plain(value);",
      "generic<Input>(value);",
      "generic?.<Output>(next);",
      "tag<Result>`value`;",
      "factory<Item>;",
      "factory<Item>?.(value);",
      "service?.method<Value>;",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [
      plain,
      generic,
      optional,
      tagged,
      instantiation,
      optionalInstantiationCall,
      chainedInstantiation,
    ] = result.program.body.map(statement => statement.expression);
    expect(plain).toMatchObject({
      type: "CallExpression",
      callee: { name: "plain" },
      arguments: [{ name: "value" }],
      optional: false,
    });
    expect(plain).not.toHaveProperty("typeArguments");
    expect(generic).toMatchObject({
      type: "CallExpression",
      callee: { name: "generic" },
      arguments: [{ name: "value" }],
      optional: false,
      typeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ typeName: { name: "Input" } }],
      },
    });
    expect(optional).toMatchObject({
      type: "ChainExpression",
      expression: {
        type: "CallExpression",
        callee: { name: "generic" },
        arguments: [{ name: "next" }],
        optional: true,
        typeArguments: {
          type: "TSTypeParameterInstantiation",
          params: [{ typeName: { name: "Output" } }],
        },
      },
    });
    expect(tagged).toMatchObject({
      type: "TaggedTemplateExpression",
      tag: { name: "tag" },
      quasi: { type: "TemplateLiteral" },
      typeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ typeName: { name: "Result" } }],
      },
    });
    expect(instantiation).toMatchObject({
      type: "TSInstantiationExpression",
      expression: { name: "factory" },
      typeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ typeName: { name: "Item" } }],
      },
    });
    expect(optionalInstantiationCall).toMatchObject({
      type: "ChainExpression",
      expression: {
        type: "CallExpression",
        optional: true,
        arguments: [{ name: "value" }],
        callee: {
          type: "TSInstantiationExpression",
          expression: { name: "factory" },
          typeArguments: {
            type: "TSTypeParameterInstantiation",
            params: [{ typeName: { name: "Item" } }],
          },
        },
      },
    });
    expect(chainedInstantiation).toMatchObject({
      type: "TSInstantiationExpression",
      expression: {
        type: "ChainExpression",
        expression: {
          type: "MemberExpression",
          object: { name: "service" },
          property: { name: "method" },
          optional: true,
        },
      },
      typeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ typeName: { name: "Value" } }],
      },
    });

    const typed = [
      generic,
      optional.expression,
      tagged,
      instantiation,
      optionalInstantiationCall.expression.callee,
      chainedInstantiation,
    ];
    expect(typed.map(expression => source.slice(expression.start, expression.end))).toEqual([
      "generic<Input>(value)",
      "generic?.<Output>(next)",
      "tag<Result>`value`",
      "factory<Item>",
      "factory<Item>",
      "service?.method<Value>",
    ]);
    expect(typed.map(expression => (
      source.slice(expression.typeArguments.start, expression.typeArguments.end)
    ))).toEqual(["<Input>", "<Output>", "<Result>", "<Item>", "<Item>", "<Value>"]);
    for (const expression of typed) {
      expect(expression.range).toEqual([expression.start, expression.end]);
      expect(expression.typeArguments.range).toEqual([
        expression.typeArguments.start,
        expression.typeArguments.end,
      ]);
    }
    expect(optional.range).toEqual([optional.start, optional.end]);
    expect(optionalInstantiationCall.range).toEqual([
      optionalInstantiationCall.start,
      optionalInstantiationCall.end,
    ]);

    const shifted = parse("factory<Item> << count;", { lang: "ts" });
    expect(shifted.diagnostics).toEqual([]);
    expect(shifted.program.body[0].expression).toMatchObject({
      type: "BinaryExpression",
      operator: "<<",
      left: {
        type: "TSInstantiationExpression",
        expression: { name: "factory" },
        typeArguments: {
          type: "TSTypeParameterInstantiation",
          params: [{ typeName: { name: "Item" } }],
        },
      },
      right: { name: "count" },
    });
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

  it("materializes TypeScript superclass type arguments", () => {
    const source = [
      "class Derived extends Base<Input> {}",
      "class Generic<Key> extends Namespace.Base<Map<Key, string>> implements Repository<Key> {}",
      "const Anonymous = class extends Base<Result<number>> {};",
      "abstract class AbstractDerived extends Base<unknown> {}",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true });

    expect(result.diagnostics).toEqual([]);
    const [derived, generic, anonymousDeclaration, abstractDerived] = result.program.body;
    expect(derived).toMatchObject({
      type: "ClassDeclaration",
      superClass: { type: "Identifier", name: "Base" },
      superTypeArguments: {
        type: "TSTypeParameterInstantiation",
        params: [{ typeName: { name: "Input" } }],
      },
    });
    expect(derived).not.toHaveProperty("typeParameters");
    expect(generic).toMatchObject({
      type: "ClassDeclaration",
      typeParameters: { params: [{ name: { name: "Key" } }] },
      superClass: {
        type: "MemberExpression",
        object: { name: "Namespace" },
        property: { name: "Base" },
      },
      superTypeArguments: {
        params: [{
          typeName: { name: "Map" },
          typeArguments: { params: [{ typeName: { name: "Key" } }, { type: "TSStringKeyword" }] },
        }],
      },
      implements: [{ expression: { name: "Repository" } }],
    });
    expect(anonymousDeclaration.declarations[0].init).toMatchObject({
      type: "ClassExpression",
      id: null,
      superClass: { name: "Base" },
      superTypeArguments: { params: [{ typeName: { name: "Result" } }] },
    });
    expect(abstractDerived).toMatchObject({
      type: "ClassDeclaration",
      abstract: true,
      superTypeArguments: { params: [{ type: "TSUnknownKeyword" }] },
    });
    const genericSuperclasses = [
      derived,
      generic,
      anonymousDeclaration.declarations[0].init,
      abstractDerived,
    ];
    expect(genericSuperclasses.map(declaration => (
      source.slice(declaration.superClass.start, declaration.superClass.end)
    ))).toEqual(["Base", "Namespace.Base", "Base", "Base"]);
    expect(genericSuperclasses.map(declaration => (
      source.slice(declaration.superTypeArguments.start, declaration.superTypeArguments.end)
    ))).toEqual(["<Input>", "<Map<Key, string>>", "<Result<number>>", "<unknown>"]);
    for (const declaration of genericSuperclasses) {
      expect(declaration.superTypeArguments.range).toEqual([
        declaration.superTypeArguments.start,
        declaration.superTypeArguments.end,
      ]);
    }

    const legacy = parse(
      "class Plain {} class Relational extends (left < middle > right) {}",
      { lang: "ts" },
    );
    expect(legacy.diagnostics).toEqual([]);
    expect(legacy.program.body[0]).not.toHaveProperty("superTypeArguments");
    expect(legacy.program.body[1]).not.toHaveProperty("superTypeArguments");

    for (const lang of ["ts", "tsx", "dts"]) {
      const typed = parse("class Derived extends Base<Input> {}", { lang });
      expect(typed.diagnostics, lang).toEqual([]);
      expect(typed.program.body[0]).toHaveProperty("superTypeArguments");
    }
    const compatibility = parse("class Derived extends Base<Input> {}", {
      typescriptJsCompatibility: true,
    });
    expect(compatibility.diagnostics).toEqual([]);
    expect(compatibility.program.body[0]).toHaveProperty("superTypeArguments");

    for (const lang of ["js", "jsx"]) {
      const standard = parse("class Derived extends Base<Input> {}", { lang });
      expect(standard.diagnostics, lang).not.toEqual([]);
      expect(standard.program.body[0], lang).not.toHaveProperty("superTypeArguments");
    }
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

  it("materializes TypeScript parameter properties", () => {
    const source = [
      "class Service extends Base {",
      "  constructor(",
      "    public readonly name?: string,",
      "    protected count = 1,",
      "    private override enabled: boolean,",
      "  ) { super(); }",
      "}",
    ].join("\n");
    const result = parse(source, { lang: "ts", range: true, semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0].body.body[0].value.params).toMatchObject([
      {
        type: "TSParameterProperty",
        accessibility: "public",
        readonly: true,
        parameter: {
          type: "Identifier",
          name: "name",
          optional: true,
          typeAnnotation: { type: "TSTypeAnnotation" },
        },
      },
      {
        type: "TSParameterProperty",
        accessibility: "protected",
        parameter: {
          type: "AssignmentPattern",
          left: { type: "Identifier", name: "count" },
        },
      },
      {
        type: "TSParameterProperty",
        accessibility: "private",
        override: true,
        parameter: { type: "Identifier", name: "enabled" },
      },
    ]);
    const property = result.program.body[0].body.body[0].value.params[0];
    expect(source.slice(property.start, property.end)).toBe("public readonly name?: string");
    expect(property.range).toEqual([property.start, property.end]);
    expect(property.parameter.start).toBeGreaterThan(property.start);
    expect(property.parameter.end).toBe(property.end);
  });

  it("recovers invalid TypeScript parameter-property contexts", () => {
    for (
      const source of [
        "function ordinary(public value: string) {}",
        "class C { method(readonly value: string) {} }",
        "class C { constructor(public value: string); }",
        "type Callback = (private value: string) => void;",
        "class C { constructor(public { value }: Source) {} }",
        "class C { constructor(public ...values: string[]) {} }",
        "class C { constructor(readonly override value: string) {} }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(JSON.stringify(result.program), source).toContain("TSParameterProperty");
    }

    const syntaxOnly = parse(
      "function ordinary(public value: string) {} class C { constructor(readonly value: string); }",
      { lang: "ts", semanticErrors: false },
    );
    expect(syntaxOnly.diagnostics).toEqual([]);
    expect(JSON.stringify(syntaxOnly.program).match(/TSParameterProperty/g)).toHaveLength(2);
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

    const newlineSource = [
      "class IHeapObjectProperty {}",
      "class IDirectChildrenMap {",
      "  hasOwnProperty(objectId: number): boolean",
      "  [objectId: number]: IHeapObjectProperty[]",
      "  next(): void",
      "  implemented(): Foo[Key]",
      "  { return value; }",
      "  tail(): void",
      "}",
    ].join("\n");
    const newline = parse(newlineSource, { lang: "ts", range: true });
    expect(newline.diagnostics).toEqual([]);
    const newlineMembers = newline.program.body[1].body.body;
    expect(newlineMembers).toMatchObject([
      {
        type: "MethodDefinition",
        value: {
          type: "TSEmptyBodyFunctionExpression",
          returnType: { typeAnnotation: { type: "TSBooleanKeyword" } },
        },
      },
      { type: "TSIndexSignature" },
      { type: "MethodDefinition", value: { type: "TSEmptyBodyFunctionExpression" } },
      {
        type: "MethodDefinition",
        value: {
          type: "FunctionExpression",
          returnType: { typeAnnotation: { type: "TSIndexedAccessType" } },
        },
      },
      { type: "MethodDefinition", value: { type: "TSEmptyBodyFunctionExpression" } },
    ]);
    expect(newlineMembers.map(member => newlineSource.slice(member.start, member.end))).toEqual([
      "hasOwnProperty(objectId: number): boolean",
      "[objectId: number]: IHeapObjectProperty[]",
      "next(): void",
      "implemented(): Foo[Key]\n  { return value; }",
      "tail(): void",
    ]);
    expect([newlineMembers[0], newlineMembers[2], newlineMembers[4]].map(method => (
      newlineSource.slice(method.value.start, method.value.end)
    ))).toEqual(["(objectId: number): boolean", "(): void", "(): void"]);
    for (const member of newlineMembers) {
      expect(member.range).toEqual([member.start, member.end]);
      if (member.type === "MethodDefinition") {
        expect(member.value.range).toEqual([member.value.start, member.value.end]);
      }
    }

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

  it("materializes explicit ambient classes across TypeScript layouts", () => {
    const source = [
      "declare class Plain { constructor(value: string); method(): void; }",
      "declare class Implemented implements Contract {}",
      "declare class Generic<T> {}",
      "declare abstract class Derived<T> extends Base<T> implements Contract<T> {",
      "  abstract method(): T;",
      "}",
      "export declare class Exported {}",
    ].join("\n");
    const result = parse(source, {
      lang: "ts",
      range: true,
      semanticErrors: true,
      sourceType: "module",
    });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      { type: "ClassDeclaration", declare: true, id: { name: "Plain" } },
      {
        type: "ClassDeclaration",
        declare: true,
        id: { name: "Implemented" },
        implements: [{ type: "TSClassImplements" }],
      },
      {
        type: "ClassDeclaration",
        declare: true,
        id: { name: "Generic" },
        typeParameters: { type: "TSTypeParameterDeclaration" },
      },
      {
        type: "ClassDeclaration",
        declare: true,
        abstract: true,
        id: { name: "Derived" },
        superTypeArguments: { type: "TSTypeParameterInstantiation" },
      },
      {
        type: "ExportNamedDeclaration",
        exportKind: "type",
        declaration: { type: "ClassDeclaration", declare: true, id: { name: "Exported" } },
      },
    ]);
    for (const statement of result.program.body) {
      const declaration = statement.declaration ?? statement;
      expect(source.slice(declaration.start, declaration.start + 7)).toBe("declare");
      expect(declaration.range).toEqual([declaration.start, declaration.end]);
    }

    const ordinary = parse("class Ordinary {} abstract class Abstract {}", { lang: "ts" });
    expect(ordinary.diagnostics).toEqual([]);
    expect(ordinary.program.body[0]).not.toHaveProperty("declare");
    expect(ordinary.program.body[1]).toMatchObject({ abstract: true });
    expect(ordinary.program.body[1]).not.toHaveProperty("declare");
  });

  it("diagnoses explicit ambient class implementations", () => {
    for (
      const source of [
        "declare class Invalid { method() {} }",
        "declare class Invalid { field = 1; }",
        "declare class Invalid { static {} }",
        "declare class Duplicate {} declare class Duplicate {}",
        "function nested() { declare class Nested {} }",
        "class Outer { method() { declare class Nested {} } }",
        "if (condition) { declare class Nested {} }",
      ]
    ) {
      const result = parse(source, { lang: "ts", semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
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

  it("allows resource declarations in TypeScript namespace statement lists", () => {
    const result = parse(
      "namespace N { using first = acquire(); } module M { using second = acquire(); }",
      { lang: "ts", semanticErrors: true, sourceType: "script" },
    );
    expect(result.diagnostics).toEqual([]);
    expect(result.program.body).toMatchObject([
      { body: { body: [{ type: "VariableDeclaration", kind: "using" }] } },
      { body: { body: [{ type: "VariableDeclaration", kind: "using" }] } },
    ]);

    const ambient = parse("declare namespace N { using resource = acquire(); }", {
      lang: "ts",
      semanticErrors: true,
      sourceType: "script",
    });
    expect(ambient.diagnostics).toContain("initializers are not allowed in ambient contexts");
    expect(ambient.diagnostics).not.toContain(
      "using declarations are not allowed in this statement context",
    );

    const invalidAwait = parse(
      "export {}; namespace N { await using resource = acquire(); "
        + "for await (using item of source) {} }",
      { lang: "ts", semanticErrors: true, sourceType: "module" },
    );
    expect(invalidAwait.diagnostics.length).toBeGreaterThanOrEqual(2);

    const asyncFunction = parse(
      "export {}; namespace N { async function f() { await using resource = acquire(); "
        + "for await (using item of source) {} } }",
      { lang: "ts", semanticErrors: true, sourceType: "module" },
    );
    expect(asyncFunction.diagnostics).toEqual([]);
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

  it("restricts declarations to statement-list positions", () => {
    for (
      const source of [
        "if (condition) let value;",
        "if (condition) class Value {}",
        "if (condition) function* generated() {}",
        "if (condition) async function asynchronous() {}",
        "if (condition); else class Alternate {}",
        "'use strict'; if (condition) function strict() {}",
        "while (condition) function nested() {}",
        "do class Nested {} while (condition);",
        "for (;;) const value = 1;",
        "for (key in object) let value;",
        "for (value of values) class Nested {}",
        "async function run() { for await (value of values) let nested; }",
        "with (object) let value;",
        "with (object) function nested() {}",
        "label: const value = 1;",
        "label: function* generated() {}",
        "label: async function asynchronous() {}",
        "if (condition) label: function nested() {}",
        "while (condition) first: second: function nested() {}",
        "if (condition) let\n[value] = values;",
      ]
    ) {
      const result = parse(source, {
        lang: "js",
        semanticErrors: true,
        sourceType: "script",
      });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    for (
      const source of [
        "for (;;) interface Contract {}",
        "for (;;) type Value = number;",
        "if (condition) declare class Ambient {}",
        "while (condition) enum Choice {}",
      ]
    ) {
      const result = parse(source, {
        lang: "ts",
        semanticErrors: true,
        sourceType: "script",
      });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    for (
      const source of [
        "label: import value from 'package';",
        "if (condition) export { value };",
      ]
    ) {
      const result = parse(source, {
        lang: "js",
        semanticErrors: true,
        sourceType: "module",
      });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.program.type).toBe("Program");
    }
  });

  it("preserves valid statements and sloppy Annex B functions", () => {
    for (
      const source of [
        "if (condition) function annexB() {}",
        "label: function annexB() {}",
        "first: second: function annexB() {}",
        "label: if (condition) function annexB() {}",
        "while (condition) var value;",
        "if (condition) { let value; class Nested {} }",
        "for (;;) { const value = 1; function nested() {} }",
        "if (condition) letValue;",
        "if (condition) let\nvalue = 1;",
        "if (condition) let\n{}",
        "if (condition) let\n\\u0076alue = 1;",
        "if (condition) async\nfunction separated() {}",
        "label: import('package');",
        "'use strict'; function topLevel() {}",
      ]
    ) {
      const result = parse(source, {
        lang: "js",
        semanticErrors: true,
        sourceType: "script",
      });
      expect(result.diagnostics, source).toEqual([]);
      expect(result.program.type).toBe("Program");
    }

    const asi = parse("if (condition) let\nvalue = 1;", {
      lang: "js",
      semanticErrors: true,
      sourceType: "script",
    });
    expect(asi.program.body).toMatchObject([
      {
        type: "IfStatement",
        consequent: { type: "ExpressionStatement", expression: { name: "let" } },
      },
      { type: "ExpressionStatement", expression: { type: "AssignmentExpression" } },
    ]);

    for (
      const source of [
        "while (condition) function nested() {}",
        "if (condition) const value = 1;",
        "if (condition) label: function nested() {}",
      ]
    ) {
      const result = parse(source, {
        lang: "js",
        semanticErrors: false,
        sourceType: "script",
      });
      expect(result.diagnostics, source).toEqual([]);
      expect(result.program.type).toBe("Program");
    }
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

  it("rejects arguments in class initializers until an ordinary function boundary", () => {
    for (
      const source of [
        "class C { field = arguments; }",
        "class C { field = () => ({ arguments }); }",
        "class C { field = async () => arguments; }",
        "class C { #field = () => () => argument\\u0073; }",
        "class C { field = ({ [arguments]() {} }); }",
        "class C { static field = typeof arguments; }",
        "class C { static { arguments; } }",
        "class C { static { class Nested { [arguments]() {} } } }",
      ]
    ) {
      const result = parse(source, { semanticErrors: true, sourceType: "script" });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }

    const typescript = parse("class C { field: unknown = () => arguments; }", {
      lang: "ts",
      semanticErrors: true,
    });
    expect(typescript.diagnostics).not.toEqual([]);
    expect(typescript.panicked).toBe(false);

    const allowed = parse(
      "function outer() { class C { [arguments] = 1; "
        + "member = object.arguments; named = ({ arguments: 1, arguments() {} }); "
        + "field = function(value = arguments) { return arguments; }; "
        + "generator = function*(value = arguments) { return arguments; }; "
        + "asynchronous = async function(value = arguments) { return arguments; }; "
        + "arrow = () => function() { return arguments; }; "
        + "static { function nested() { return arguments; } "
        + "class Nested { method() { return arguments; } } } } }",
      { semanticErrors: true, sourceType: "script" },
    );
    expect(allowed.diagnostics).toEqual([]);

    expect(
      parse("class C { field = arguments; static { arguments; } }", {
        semanticErrors: false,
      }).diagnostics,
    ).toEqual([]);
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

  it("returns ESTree resource declarations without confusing contextual using expressions", () => {
    const source = [
      "using sync = acquire();",
      "for (using item of items) {}",
      "class ResourceOwner { async method() { await using item = acquire(); } }",
      "async function consume() {",
      "  await using asyncResource = acquire();",
      "  for await (await using item of items) {}",
      "}",
    ].join("\n");
    const result = parse(source, { range: true, semanticErrors: true });

    expect(result.diagnostics).toEqual([]);
    expect(result.program.body[0]).toMatchObject({
      type: "VariableDeclaration",
      kind: "using",
      declarations: [{ id: { name: "sync" }, init: { type: "CallExpression" } }],
      range: [0, 23],
    });
    expect(result.program.body[1]).toMatchObject({
      type: "ForOfStatement",
      await: false,
      left: { type: "VariableDeclaration", kind: "using" },
    });
    expect(result.program.body[3].body.body).toMatchObject([
      { type: "VariableDeclaration", kind: "await using" },
      {
        type: "ForOfStatement",
        await: true,
        left: { type: "VariableDeclaration", kind: "await using" },
      },
    ]);

    const expressions = parse(
      "var using, value, key, object, Type, let; "
        + "using; using(value); using[key] = value; using in object; "
        + "let in object; let instanceof Type; for (let in object) {} "
        + "async function f() { await using\nlet in object; }",
      { semanticErrors: true, sourceType: "script" },
    );
    expect(expressions.diagnostics).toEqual([]);
    expect(expressions.program.body).toContainEqual(
      expect.objectContaining({
        type: "ForInStatement",
        left: expect.objectContaining({ name: "let" }),
      }),
    );
  });

  it("supports TypeScript resource modifiers and reports JavaScript early errors", () => {
    const typescript = parse(
      "declare using sync: Disposable; export await using asyncValue: AsyncDisposable = value;",
      { lang: "ts", semanticErrors: false, sourceType: "module" },
    );
    expect(typescript.diagnostics).toEqual([]);
    expect(typescript.program.body).toMatchObject([
      { type: "VariableDeclaration", declare: true, kind: "using" },
      {
        type: "ExportNamedDeclaration",
        declaration: { type: "VariableDeclaration", kind: "await using" },
      },
    ]);

    for (
      const [source, sourceType] of [
        ["using resource = value;", "script"],
        ["function f() { await using resource = value; }", "module"],
        ["if (ready) using resource = value;", "module"],
        ["switch (key) { case 0: using resource = value; }", "module"],
        ["for (using resource in values) {}", "module"],
        ["for (using resource = value of values) {}", "module"],
        ["for (let.member of values) {}", "script"],
        ["function f() { using enum = null; }", "module"],
        ["async function f() { await using enum = null; }", "module"],
        [
          "async function outer() { class C { static { await using resource = acquire(); "
          + "await using\nitem = source; "
          + "for await (item of source) {} for await (using other of source) {} } } }",
          "module",
        ],
        ["async function f() { for await (using resource = value;;) {} }", "module"],
        ["function f() { for await (using resource of values) {} }", "module"],
        ["export using resource = value;", "module"],
      ]
    ) {
      const invalid = parse(source, { semanticErrors: true, sourceType });
      expect(invalid.diagnostics, source).not.toEqual([]);
      expect(invalid.panicked, source).toBe(false);
    }

    expect(
      parse("using resource = value;", {
        semanticErrors: true,
        sourceType: "commonjs",
      }).diagnostics,
    ).toEqual([]);

    const contextualYield = parse(
      "function sync() { using yield = null; for (using yield of values) {} } "
        + "async function asynchronous() { await using yield = null; }",
      { semanticErrors: true, sourceType: "script" },
    );
    expect(contextualYield.diagnostics).toEqual([]);

    for (
      const source of [
        "'use strict'; function f() { using yield = null; } function g(yield) {}",
        "function f(yield) { 'use strict'; }",
        "function* f() { using yield = null; }",
      ]
    ) {
      expect(parse(source, { semanticErrors: true, sourceType: "script" }).diagnostics, source)
        .not.toEqual([]);
    }

    for (
      const source of [
        "async function f() { for await (item in source) {} }",
        "async function f() { for await (using item in source) {} }",
      ]
    ) {
      expect(parse(source, { semanticErrors: false, sourceType: "module" }).diagnostics, source)
        .not.toEqual([]);
    }

    const typescriptSemanticFree = parse(
      "function f() { for await (item of source) {} "
        + "for await (using resource of source) {} "
        + "for await (await using asyncResource of source) {} }",
      { lang: "ts", semanticErrors: false, sourceType: "script" },
    );
    expect(typescriptSemanticFree.diagnostics).toEqual([]);
  });
});
