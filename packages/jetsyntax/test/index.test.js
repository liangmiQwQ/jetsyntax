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
      "class C { get #value() {} }",
    ];

    for (const source of sources) {
      const result = parse(source, { semanticErrors: true });
      expect(result.diagnostics, source).not.toEqual([]);
      expect(result.panicked, source).toBe(false);
      expect(result.program.type, source).toBe("Program");
    }
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
});
