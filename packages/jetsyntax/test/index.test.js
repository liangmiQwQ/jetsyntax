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

  it("keeps unsupported function return forms diagnostic", () => {
    for (
      const [source, options] of [
        ["function predicate(value: unknown): value is string { return true; }", { lang: "ts" }],
        ["function assertion(value: unknown): asserts value {}", { lang: "ts" }],
        ["function overload(): string;", { lang: "ts" }],
        ["declare function declared(): string;", { lang: "ts" }],
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

  it("keeps unsupported method return forms diagnostic", () => {
    for (
      const [source, options] of [
        ["class C { predicate(value): value is string {} }", { lang: "ts" }],
        ["class C { assertion(value): asserts value {} }", { lang: "ts" }],
        ["class C { overload(): string; }", { lang: "ts" }],
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
