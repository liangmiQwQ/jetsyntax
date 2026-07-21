import { describe, expect, it } from "vitest";

import { decodeTape, decodeTrustedTape } from "../decoder.js";

const KIND_NODE = 1;
const KIND_LIST = 2;
const KIND_NULL = 3;
const KIND_BOOL = 4;
const KIND_INLINE_U32 = 5;
const KIND_SOURCE_SLICE = 8;
const KIND_POOL_STRING = 9;
const KIND_SHIFT = 28;
const REFERENCE_MARKER = 0x0800_0000;

class HandcraftedTape {
  words = new Array(12).fill(0);
  pool = [];
  references = [];

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
    this.references.push(...items);
    return this.record([(KIND_LIST << KIND_SHIFT) >>> 0, 3 + items.length, items.length, ...items]);
  }

  node(tag, start, end, fields) {
    this.references.push(...fields);
    return this.record([
      ((KIND_NODE << KIND_SHIFT) | tag) >>> 0,
      5 + fields.length,
      start,
      end,
      fields.length,
      ...fields,
    ]);
  }

  finish(root, { referenceMarkers = false } = {}) {
    const recordEnd = this.words.length;
    if (referenceMarkers) {
      for (const reference of this.references) this.words[reference] |= REFERENCE_MARKER;
    }
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
    this.words[3] = referenceMarkers ? 7 : 3;
    this.words[4] = this.words.length;
    this.words[5] = recordEnd;
    this.words[6] = this.pool.length;
    this.words[7] = root;
    this.words[8] = this.words[root + 3];
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

  it("decodes reference-marked records without exposing the marker as data", () => {
    const tape = new HandcraftedTape();
    const name = tape.string("answer");
    const identifier = tape.node(2, 0, 0, [name]);
    const statement = tape.node(5, 0, 0, [identifier]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(1);
    const program = tape.node(1, 0, 0, [body, sourceType]);
    const encoded = tape.finish(program, { referenceMarkers: true });

    expect(encoded[sourceType] & REFERENCE_MARKER).toBe(REFERENCE_MARKER);
    expect(encoded[program] & REFERENCE_MARKER).toBe(0);
    expect(decodeTape("", encoded)).toEqual(decodeTrustedTape("", encoded));
    expect(decodeTrustedTape("", encoded)).toMatchObject({
      type: "Program",
      sourceType: "module",
      body: [{ expression: { name: "answer" } }],
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

    const encoded = tape.finish(program);
    const decoded = decodeTape(source, encoded, { range: true });
    expect(decodeTrustedTape(source, encoded, { range: true })).toEqual(decoded);
    expect(decoded.end).toBe(3);
    expect(decoded.body[0].expression).toMatchObject({ name: "<invalid>", start: 0, end: 2 });

    const invalidSourceLength = encoded.slice();
    invalidSourceLength[8] = source.length;
    expect(() => decodeTape(source, invalidSourceLength)).toThrow(
      "source UTF-8 length does not match JetSyntax input",
    );
  });

  it("uses equal UTF-8 and UTF-16 lengths only for ASCII input", () => {
    for (
      const [source, ascii] of [
        ["const answer = 42;", true],
        ["\u00E9", false],
        ["\u{1F4A5}", false],
        ["\uD800", false],
      ]
    ) {
      const byteLength = new TextEncoder().encode(source).length;
      expect(byteLength === source.length).toBe(ascii);

      const tape = new HandcraftedTape();
      const body = tape.list([]);
      const sourceType = tape.integer(0);
      const program = tape.node(1, 0, byteLength, [body, sourceType]);
      const encoded = tape.finish(program);
      const decoded = decodeTape(source, encoded, { range: true });

      expect(decodeTrustedTape(source, encoded, { range: true })).toEqual(decoded);
      expect(decoded).toMatchObject({ start: 0, end: source.length, range: [0, source.length] });
    }
  });

  it("rejects a forged ASCII-length header for non-ASCII input", () => {
    const source = "\u00E9";
    const tape = new HandcraftedTape();
    const body = tape.list([]);
    const sourceType = tape.integer(0);
    const program = tape.node(1, 0, source.length, [body, sourceType]);

    expect(() => decodeTape(source, tape.finish(program))).toThrow(
      "source UTF-8 length does not match JetSyntax input",
    );
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
          typeArguments: null,
        },
      },
    });
  });

  it("decodes legacy and annotated function field counts", () => {
    const source = "function plain() {} function typed(): number {}";
    const tape = new HandcraftedTape();
    const emptyParameters = tape.list([]);
    const emptyBody = tape.node(6, 17, 19, [tape.list([])]);
    const legacy = tape.node(25, 0, 19, [
      tape.null(),
      emptyParameters,
      emptyBody,
      tape.boolean(false),
      tape.boolean(false),
    ]);

    const returnType = tape.node(512, 36, 44, [tape.node(548, 38, 44, [])]);
    const annotated = tape.node(26, 20, source.length, [
      tape.null(),
      tape.list([]),
      tape.node(6, 45, 47, [tape.list([])]),
      tape.boolean(false),
      tape.boolean(false),
      returnType,
    ]);
    const program = tape.node(1, 0, source.length, [tape.list([legacy, annotated]), tape.integer(0)]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0]).toMatchObject({
      type: "FunctionDeclaration",
      params: [],
      generator: false,
      async: false,
    });
    expect(decoded.body[0]).not.toHaveProperty("returnType");
    expect(decoded.body[1]).toMatchObject({
      type: "FunctionExpression",
      returnType: {
        type: "TSTypeAnnotation",
        start: 36,
        end: 44,
        typeAnnotation: { type: "TSNumberKeyword", start: 38, end: 44 },
      },
    });
  });

  it("rejects malformed function field counts", () => {
    for (
      const [tag, fields, expected] of [
        [25, ["null", "list", "body", false], "4"],
        [26, ["null", "list", "body", false, false, "null", "null"], "7"],
      ]
    ) {
      const tape = new HandcraftedTape();
      const values = fields.map(value => {
        if (value === "null") return tape.null();
        if (value === "list") return tape.list([]);
        if (value === "body") return tape.node(6, 0, 0, [tape.list([])]);
        return tape.boolean(value);
      });
      const encoded = tape.finish(tape.node(tag, 0, 0, values));

      expect(() => decodeTape("", encoded)).toThrow(
        `invalid ${tag === 25 ? "FunctionDeclaration" : "FunctionExpression"} field count ${expected}; expected 5 or 6`,
      );
      expect(() => decodeTrustedTape("", encoded)).toThrow(
        `invalid ${tag === 25 ? "FunctionDeclaration" : "FunctionExpression"} field count ${expected}; expected 5 or 6`,
      );
    }
  });

  it("decodes compound TypeScript type and declaration records", () => {
    const tape = new HandcraftedTape();
    const aliasId = tape.node(2, 0, 0, [tape.string("Shape")]);
    const typeParameterName = tape.node(2, 0, 0, [tape.string("T")]);
    const typeParameter = tape.node(534, 0, 0, [
      typeParameterName,
      tape.boolean(false),
      tape.boolean(false),
      tape.boolean(false),
      tape.null(),
      tape.null(),
    ]);
    const typeParameters = tape.node(541, 0, 0, [tape.list([typeParameter])]);

    const objectName = tape.node(2, 0, 0, [tape.string("T")]);
    const objectType = tape.node(513, 0, 0, [objectName, tape.null()]);
    const indexType = tape.node(548, 0, 0, []);
    const indexed = tape.node(532, 0, 0, [objectType, indexType]);
    const operator = tape.node(533, 0, 0, [tape.string("readonly"), indexed]);
    const parenthesized = tape.node(531, 0, 0, [operator]);
    const label = tape.node(2, 0, 0, [tape.string("value")]);
    const tupleMember = tape.node(538, 0, 0, [label, parenthesized, tape.boolean(true)]);
    const tuple = tape.node(519, 0, 0, [tape.list([tupleMember])]);
    const propertyAnnotation = tape.node(512, 0, 0, [tuple]);
    const propertyKey = tape.node(2, 0, 0, [tape.string("items")]);
    const property = tape.node(535, 0, 0, [
      propertyKey,
      propertyAnnotation,
      tape.boolean(false),
      tape.boolean(false),
      tape.boolean(true),
    ]);

    const returnName = tape.node(2, 0, 0, [tape.string("Promise")]);
    const argumentName = tape.node(2, 0, 0, [tape.string("T")]);
    const argumentReference = tape.node(513, 0, 0, [argumentName, tape.null()]);
    const typeArguments = tape.node(542, 0, 0, [tape.list([argumentReference])]);
    const returnReference = tape.node(513, 0, 0, [returnName, typeArguments]);
    const returnType = tape.node(512, 0, 0, [returnReference]);
    const methodKey = tape.node(2, 0, 0, [tape.string("get")]);
    const method = tape.node(536, 0, 0, [
      methodKey,
      tape.null(),
      tape.list([]),
      returnType,
      tape.boolean(false),
      tape.boolean(false),
    ]);

    const inferredName = tape.node(2, 0, 0, [tape.string("Element")]);
    const inferredParameter = tape.node(534, 0, 0, [
      inferredName,
      tape.boolean(false),
      tape.boolean(false),
      tape.boolean(false),
      tape.null(),
      tape.null(),
    ]);
    const inferredType = tape.node(556, 0, 0, [inferredParameter]);
    const inferredAnnotation = tape.node(512, 0, 0, [inferredType]);
    const inferredKey = tape.node(2, 0, 0, [tape.string("element")]);
    const inferredProperty = tape.node(535, 0, 0, [
      inferredKey,
      inferredAnnotation,
      tape.boolean(false),
      tape.boolean(false),
      tape.boolean(false),
    ]);
    const typeLiteral = tape.node(523, 0, 0, [tape.list([property, method, inferredProperty])]);
    const alias = tape.node(525, 0, 0, [aliasId, typeParameters, typeLiteral]);

    const interfaceId = tape.node(2, 0, 0, [tape.string("Repository")]);
    const heritageExpression = tape.node(2, 0, 0, [tape.string("Base")]);
    const heritage = tape.node(558, 0, 0, [heritageExpression, tape.null()]);
    const interfaceBody = tape.node(539, 0, 0, [tape.list([])]);
    const interfaceDeclaration = tape.node(524, 0, 0, [
      interfaceId,
      tape.null(),
      tape.list([heritage]),
      interfaceBody,
    ]);

    const outerId = tape.node(2, 0, 0, [tape.string("Library")]);
    const innerId = tape.node(2, 0, 0, [tape.string("Core")]);
    const moduleId = tape.node(514, 0, 0, [outerId, innerId]);
    const moduleBlock = tape.node(540, 0, 0, [tape.list([])]);
    const moduleDeclaration = tape.node(527, 0, 0, [
      moduleId,
      moduleBlock,
      tape.boolean(false),
      tape.integer(0),
    ]);

    const enumId = tape.node(2, 0, 0, [tape.string("Choice")]);
    const memberId = tape.node(2, 0, 0, [tape.string("First")]);
    const initializer = tape.node(4, 0, 0, [tape.string("1"), tape.integer(0)]);
    const enumMember = tape.node(537, 0, 0, [memberId, initializer]);
    const enumBody = tape.node(557, 0, 0, [tape.list([enumMember])]);
    const enumDeclaration = tape.node(526, 0, 0, [
      enumId,
      enumBody,
      tape.boolean(true),
      tape.boolean(false),
    ]);

    const mappedAliasId = tape.node(2, 0, 0, [tape.string("Mapped")]);
    const mappedKey = tape.node(2, 0, 0, [tape.string("Key")]);
    const mappedConstraint = tape.node(550, 0, 0, []);
    const mappedAnnotation = tape.node(548, 0, 0, []);
    const mappedType = tape.node(522, 0, 0, [
      mappedKey,
      mappedConstraint,
      tape.null(),
      mappedAnnotation,
      tape.boolean(true),
      tape.boolean(true),
    ]);
    const mappedAlias = tape.node(525, 0, 0, [mappedAliasId, tape.null(), mappedType]);
    const program = tape.node(1, 0, 0, [
      tape.list([alias, enumDeclaration, interfaceDeclaration, moduleDeclaration, mappedAlias]),
      tape.integer(1),
    ]);

    const decoded = decodeTape("", tape.finish(program));
    expect(decoded.body[0]).toMatchObject({
      type: "TSTypeAliasDeclaration",
      declare: false,
      typeParameters: {
        type: "TSTypeParameterDeclaration",
        params: [{ type: "TSTypeParameter", name: { type: "Identifier", name: "T" } }],
      },
      typeAnnotation: {
        type: "TSTypeLiteral",
        members: [
          {
            type: "TSPropertySignature",
            readonly: true,
            accessibility: null,
            static: false,
            typeAnnotation: {
              typeAnnotation: {
                type: "TSTupleType",
                elementTypes: [
                  {
                    type: "TSNamedTupleMember",
                    optional: true,
                    elementType: {
                      type: "TSParenthesizedType",
                      typeAnnotation: {
                        type: "TSTypeOperator",
                        operator: "readonly",
                        typeAnnotation: { type: "TSIndexedAccessType" },
                      },
                    },
                  },
                ],
              },
            },
          },
          {
            type: "TSMethodSignature",
            key: { name: "get" },
            kind: "method",
            params: [],
            accessibility: null,
            readonly: false,
            static: false,
            returnType: {
              typeAnnotation: {
                typeArguments: {
                  type: "TSTypeParameterInstantiation",
                  params: [{ type: "TSTypeReference", typeName: { name: "T" } }],
                },
              },
            },
          },
          {
            type: "TSPropertySignature",
            typeAnnotation: {
              typeAnnotation: {
                type: "TSInferType",
                typeParameter: { name: { name: "Element" } },
              },
            },
          },
        ],
      },
    });
    expect(decoded.body[1]).toMatchObject({
      type: "TSEnumDeclaration",
      const: true,
      body: {
        type: "TSEnumBody",
        members: [{ type: "TSEnumMember", id: { name: "First" }, initializer: { value: 1 } }],
      },
    });
    expect(decoded.body[2]).toMatchObject({
      type: "TSInterfaceDeclaration",
      declare: false,
      extends: [{ type: "TSInterfaceHeritage", expression: { name: "Base" } }],
      body: { type: "TSInterfaceBody", body: [] },
    });
    expect(decoded.body[3]).toMatchObject({
      type: "TSModuleDeclaration",
      kind: "namespace",
      id: {
        type: "TSQualifiedName",
        left: { name: "Library" },
        right: { name: "Core" },
      },
      body: { type: "TSModuleBlock", body: [] },
    });
    expect(decoded.body[4]).toMatchObject({
      type: "TSTypeAliasDeclaration",
      typeAnnotation: {
        type: "TSMappedType",
        key: { type: "Identifier", name: "Key" },
        constraint: { type: "TSStringKeyword" },
        nameType: null,
        typeAnnotation: { type: "TSNumberKeyword" },
        readonly: true,
        optional: true,
      },
    });
  });

  it("omits an absent mapped type readonly modifier", () => {
    const tape = new HandcraftedTape();
    const aliasId = tape.node(2, 0, 0, [tape.string("Mapped")]);
    const mappedKey = tape.node(2, 0, 0, [tape.string("Key")]);
    const mappedConstraint = tape.node(550, 0, 0, []);
    const mappedAnnotation = tape.node(548, 0, 0, []);
    const mappedType = tape.node(522, 0, 0, [
      mappedKey,
      mappedConstraint,
      tape.null(),
      mappedAnnotation,
      tape.null(),
      tape.boolean(false),
    ]);
    const alias = tape.node(525, 0, 0, [aliasId, tape.null(), mappedType]);
    const program = tape.node(1, 0, 0, [tape.list([alias]), tape.integer(1)]);

    const decoded = decodeTape("", tape.finish(program));
    const typeAnnotation = decoded.body[0].typeAnnotation;
    expect(typeAnnotation).toMatchObject({
      type: "TSMappedType",
      optional: false,
    });
    expect(Object.hasOwn(typeAnnotation, "readonly")).toBe(false);
  });

  it("decodes TypeScript expression wrappers with Babel 8 field order", () => {
    const source = "input! as Model satisfies Constraint; <number>input;";
    const tape = new HandcraftedTape();
    const input = tape.node(2, 0, 5, [tape.string("input")]);
    const nonNull = tape.node(530, 0, 6, [input]);
    const model = tape.node(2, 10, 15, [tape.string("Model")]);
    const modelType = tape.node(513, 10, 15, [model, tape.null()]);
    const asExpression = tape.node(528, 0, 15, [nonNull, modelType]);
    const constraint = tape.node(2, 26, 36, [tape.string("Constraint")]);
    const constraintType = tape.node(513, 26, 36, [constraint, tape.null()]);
    const satisfies = tape.node(529, 0, 36, [asExpression, constraintType]);
    const satisfiesStatement = tape.node(5, 0, 36, [satisfies]);

    const numberType = tape.node(548, 39, 45, []);
    const assertedInput = tape.node(2, 46, 51, [tape.string("input")]);
    const typeAssertion = tape.node(560, 38, 51, [numberType, assertedInput]);
    const assertionStatement = tape.node(5, 38, 52, [typeAssertion]);
    const program = tape.node(1, 0, 52, [
      tape.list([satisfiesStatement, assertionStatement]),
      tape.integer(1),
    ]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0].expression).toMatchObject({
      type: "TSSatisfiesExpression",
      expression: {
        type: "TSAsExpression",
        expression: {
          type: "TSNonNullExpression",
          expression: { type: "Identifier", name: "input" },
        },
        typeAnnotation: {
          type: "TSTypeReference",
          typeName: { type: "Identifier", name: "Model" },
          typeArguments: null,
        },
      },
      typeAnnotation: {
        type: "TSTypeReference",
        typeName: { type: "Identifier", name: "Constraint" },
        typeArguments: null,
      },
    });
    expect(decoded.body[1].expression).toMatchObject({
      type: "TSTypeAssertion",
      typeAnnotation: { type: "TSNumberKeyword" },
      expression: { type: "Identifier", name: "input" },
    });
  });

  it("decodes TypeScript export assignment and namespace export records", () => {
    const source = "export = value; export as namespace Library;";
    const tape = new HandcraftedTape();
    const value = tape.node(2, 9, 14, [tape.source(9, 14)]);
    const assignment = tape.node(561, 0, 15, [value]);
    const library = tape.node(2, 36, 43, [tape.source(36, 43)]);
    const namespaceExport = tape.node(562, 16, 44, [library]);
    const program = tape.node(1, 0, 44, [
      tape.list([assignment, namespaceExport]),
      tape.integer(1),
    ]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body).toMatchObject([
      {
        type: "TSExportAssignment",
        expression: { type: "Identifier", name: "value" },
      },
      {
        type: "TSNamespaceExportDeclaration",
        id: { type: "Identifier", name: "Library" },
      },
    ]);
  });

  it("decodes TypeScript import-equals records", () => {
    const source = "import A = B.C; import type a = require(\"a\");";
    const tape = new HandcraftedTape();
    const alias = tape.node(2, 7, 8, [tape.string("A")]);
    const namespace = tape.node(2, 11, 12, [tape.string("B")]);
    const member = tape.node(2, 13, 14, [tape.string("C")]);
    const qualified = tape.node(514, 11, 14, [namespace, member]);
    const internal = tape.node(563, 0, 15, [alias, qualified, tape.integer(0)]);
    const typeAlias = tape.node(2, 28, 29, [tape.string("a")]);
    const literal = tape.node(4, 40, 43, [tape.string("\"a\""), tape.integer(1)]);
    const external = tape.node(564, 32, 44, [literal]);
    const typeImport = tape.node(563, 16, 45, [typeAlias, external, tape.integer(1)]);
    const program = tape.node(1, 0, source.length, [
      tape.list([internal, typeImport]),
      tape.integer(1),
    ]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body).toMatchObject([
      {
        type: "TSImportEqualsDeclaration",
        importKind: "value",
        id: { type: "Identifier", name: "A" },
        moduleReference: {
          type: "TSQualifiedName",
          left: { type: "Identifier", name: "B" },
          right: { type: "Identifier", name: "C" },
        },
      },
      {
        type: "TSImportEqualsDeclaration",
        importKind: "type",
        id: { type: "Identifier", name: "a" },
        moduleReference: {
          type: "TSExternalModuleReference",
          expression: { type: "Literal", value: "a", raw: "\"a\"" },
        },
      },
    ]);
  });

  it("bounds recovery patterns that temporarily wrap expression nodes", () => {
    const source = "value";
    const tape = new HandcraftedTape();
    const name = tape.source(0, 5);
    const identifier = tape.node(2, 0, 5, [name]);
    const pattern = tape.node(53, 0, 5, [identifier]);
    const statement = tape.node(5, 0, 5, [pattern]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(0);
    const program = tape.node(1, 0, 5, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0].expression).toMatchObject({
      type: "ArrayPattern",
      elements: [{ type: "Identifier", name: "value" }],
    });
  });

  it("materializes an unterminated regular expression without throwing", () => {
    const source = "/unterminated";
    const tape = new HandcraftedTape();
    const raw = tape.source(0, source.length);
    const regexpKind = tape.integer(6);
    const literal = tape.node(4, 0, source.length, [raw, regexpKind]);
    const statement = tape.node(5, 0, source.length, [literal]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(0);
    const program = tape.node(1, 0, source.length, [body, sourceType]);

    const decoded = decodeTape(source, tape.finish(program));
    expect(decoded.body[0].expression).toMatchObject({
      type: "Literal",
      regex: { pattern: "unterminated", flags: "" },
    });
  });

  it("falls back to iterative decoding for deeply nested valid tapes", () => {
    const source = "value";
    const tape = new HandcraftedTape();
    const name = tape.string(source);
    let expression = tape.node(2, 0, source.length, [name]);
    const depth = 20_000;
    for (let index = 0; index < depth; index++) {
      expression = tape.node(72, 0, source.length, [expression]);
    }
    const statement = tape.node(5, 0, source.length, [expression]);
    const body = tape.list([statement]);
    const sourceType = tape.integer(0);
    const program = tape.node(1, 0, source.length, [body, sourceType]);

    const decoded = decodeTrustedTape(source, tape.finish(program));
    let current = decoded.body[0].expression;
    for (let index = 0; index < depth; index++) {
      expect(current.type).toBe("ParenthesizedExpression");
      current = current.expression;
    }
    expect(current).toMatchObject({ type: "Identifier", name: "value" });
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
    expect(() => decodeTrustedTape("", tape.finish(root))).toThrow(
      "invalid JetSyntax backward reference",
    );
  });
});
