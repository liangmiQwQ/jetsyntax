import { describe, expect, it } from "vitest";

import { decodeTape } from "../decoder.js";

const KIND_NODE = 1;
const KIND_LIST = 2;
const KIND_NULL = 3;
const KIND_BOOL = 4;
const KIND_INLINE_U32 = 5;
const KIND_SOURCE_SLICE = 8;
const KIND_POOL_STRING = 9;
const KIND_SHIFT = 28;

class HandcraftedTape {
  words = new Array(12).fill(0);
  pool = [];

  null() {
    return this.record([(KIND_NULL << KIND_SHIFT) >>> 0]);
  }

  boolean(value) {
    return this.record([((KIND_BOOL << KIND_SHIFT) | Number(value)) >>> 0]);
  }

  integer(value) {
    return this.record([((KIND_INLINE_U32 << KIND_SHIFT) | value) >>> 0]);
  }

  source(start, end) {
    return this.record([(KIND_SOURCE_SLICE << KIND_SHIFT) >>> 0, start, end]);
  }

  string(value) {
    const bytes = new TextEncoder().encode(value);
    const start = this.pool.length;
    this.pool.push(...bytes);
    return this.record([(KIND_POOL_STRING << KIND_SHIFT) >>> 0, start, bytes.length]);
  }

  list(items) {
    return this.record([(KIND_LIST << KIND_SHIFT) >>> 0, 3 + items.length, items.length, ...items]);
  }

  node(tag, start, end, fields) {
    return this.record([
      ((KIND_NODE << KIND_SHIFT) | tag) >>> 0,
      5 + fields.length,
      start,
      end,
      fields.length,
      ...fields,
    ]);
  }

  finish(root) {
    const recordEnd = this.words.length;
    for (let index = 0; index < this.pool.length; index += 4) {
      this.words.push(
        (this.pool[index] ?? 0)
          | ((this.pool[index + 1] ?? 0) << 8)
          | ((this.pool[index + 2] ?? 0) << 16)
          | ((this.pool[index + 3] ?? 0) << 24),
      );
    }
    this.words[0] = 0x4A53_5450;
    this.words[1] = 1;
    this.words[2] = 12;
    this.words[3] = 3;
    this.words[4] = this.words.length;
    this.words[5] = recordEnd;
    this.words[6] = this.pool.length;
    this.words[7] = root;
    this.words[8] = 0;
    this.words[9] = 0;
    this.words[10] = 1;
    return Uint32Array.from(this.words);
  }

  record(words) {
    const offset = this.words.length;
    this.words.push(...words);
    return offset;
  }
}

describe("decodeTape", () => {
  it("decodes the documented JavaScript schema and source slices", () => {
    const source = "const answer = 42;";
    const tape = new HandcraftedTape();
    const name = tape.source(6, 12);
    const identifier = tape.node(2, 6, 12, [name]);
    const raw = tape.source(15, 17);
    const numberKind = tape.integer(0);
    const literal = tape.node(4, 15, 17, [raw, numberKind]);
    const declarator = tape.node(29, 6, 17, [identifier, literal]);
    const declarations = tape.list([declarator]);
    const declarationKind = tape.integer(2);
    const declaration = tape.node(28, 0, 18, [declarations, declarationKind]);
    const body = tape.list([declaration]);
    const sourceType = tape.integer(1);
    const program = tape.node(1, 0, 18, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program), { range: true });
    expect(decoded).toMatchObject({
      type: "Program",
      start: 0,
      end: 18,
      range: [0, 18],
      sourceType: "module",
      body: [{
        type: "VariableDeclaration",
        kind: "const",
        declarations: [{
          type: "VariableDeclarator",
          id: { type: "Identifier", name: "answer" },
          init: { type: "Literal", value: 42, raw: "42" },
        }],
      }],
    });
  });

  it("decodes pooled strings and converts UTF-8 spans to UTF-16 offsets", () => {
    const source = "💥;";
    const tape = new HandcraftedTape();
    const name = tape.string("<invalid>");
    const identifier = tape.node(2, 0, 4, [name]);
    const statement = tape.node(5, 0, 5, [identifier]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(0);
    const program = tape.node(1, 0, 5, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.end).toBe(3);
    expect(decoded.body[0].expression).toMatchObject({ name: "<invalid>", start: 0, end: 2 });
  });

  it("decodes the emitted JSX schema", () => {
    const source = "<x />";
    const tape = new HandcraftedTape();
    const nameText = tape.source(1, 2);
    const name = tape.node(256, 1, 2, [nameText]);
    const attributes = tape.list([]);
    const selfClosing = tape.boolean(true);
    const opening = tape.node(261, 0, 5, [name, attributes, selfClosing]);
    const closing = tape.null();
    const children = tape.list([]);
    const element = tape.node(259, 0, 5, [opening, closing, children]);
    const statement = tape.node(5, 0, 5, [element]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(1);
    const program = tape.node(1, 0, 5, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0].expression).toMatchObject({
      type: "JSXElement",
      closingElement: null,
      children: [],
      openingElement: {
        type: "JSXOpeningElement",
        name: { type: "JSXIdentifier", name: "x" },
        attributes: [],
        selfClosing: true,
      },
    });
  });

  it("decodes the emitted TypeScript annotation schema", () => {
    const source = "value: NS.Type";
    const tape = new HandcraftedTape();
    const bindingName = tape.source(0, 5);
    const namespaceName = tape.source(7, 9);
    const namespace = tape.node(2, 7, 9, [namespaceName]);
    const typeNameText = tape.source(10, 14);
    const typeName = tape.node(2, 10, 14, [typeNameText]);
    const qualified = tape.node(514, 7, 14, [namespace, typeName]);
    const parameters = tape.null();
    const reference = tape.node(513, 7, 14, [qualified, parameters]);
    const annotation = tape.node(512, 7, 14, [reference]);
    const optional = tape.boolean(false);
    const binding = tape.node(2, 0, 14, [bindingName, annotation, optional]);
    const statement = tape.node(5, 0, 14, [binding]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(1);
    const program = tape.node(1, 0, 14, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0].expression).toMatchObject({
      type: "Identifier",
      name: "value",
      optional: false,
      typeAnnotation: {
        type: "TSTypeAnnotation",
        typeAnnotation: {
          type: "TSTypeReference",
          typeName: {
            type: "TSQualifiedName",
            left: { name: "NS" },
            right: { name: "Type" },
          },
          typeParameters: null,
        },
      },
    });
  });

  it("fails loudly for unsupported, unknown, and malformed tags", () => {
    for (const [tag, message] of [[260, "unsupported"], [4096, "unknown"]]) {
      const tape = new HandcraftedTape();
      const root = tape.node(tag, 0, 0, []);
      expect(() => decodeTape("", tape.finish(root))).toThrow(`${message} JetSyntax node tag ${tag}`);
    }

    const tape = new HandcraftedTape();
    const child = tape.null();
    const root = tape.node(1, 0, 0, [child, child + 1]);
    expect(() => decodeTape("", tape.finish(root))).toThrow("invalid JetSyntax backward reference");
  });
});
