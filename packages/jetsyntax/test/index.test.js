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

    expect(result.program.body[0].declarations[0].id.right).toMatchObject({
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
    const [plain, computed, stream] = result.program.body[0].declarations[0].id.right.properties;
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
});
