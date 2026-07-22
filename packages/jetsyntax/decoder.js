const MAGIC = 0x4A53_5450;
const FORMAT_VERSION = 1;
const HEADER_WORDS = 12;

const KIND_SHIFT = 28;
const KIND_MASK = 0xF000_0000;
const NODE_FLAGS_MASK = 0x00FF_0000;
const NODE_TAG_MASK = 0x0000_FFFF;
const REFERENCE_MARKER = 0x0800_0000;
const INLINE_U32_MASK = 0x0FFF_FFFF;
const MARKED_INLINE_U32_MASK = INLINE_U32_MASK & ~REFERENCE_MARKER;

const FLAG_SOURCE_UTF8 = 1 << 0;
const FLAG_POOL_UTF8 = 1 << 1;
const FLAG_REFERENCE_MARKERS = 1 << 2;
const WIRE_FLAGS = FLAG_SOURCE_UTF8 | FLAG_POOL_UTF8;
const PARSER_WIRE_FLAGS = WIRE_FLAGS | FLAG_REFERENCE_MARKERS;

const KIND_NODE = 1;
const KIND_LIST = 2;
const KIND_NULL = 3;
const KIND_BOOL = 4;
const KIND_INLINE_U32 = 5;
const KIND_U32 = 6;
const KIND_F64 = 7;
const KIND_SOURCE_SLICE = 8;
const KIND_POOL_STRING = 9;

const HOST_LITTLE_ENDIAN = new Uint8Array(Uint32Array.of(0x0102_0304).buffer)[0] === 4;

const HEADER_TOTAL_WORDS = 4;
const HEADER_FLAGS = 3;
const HEADER_RECORD_END = 5;
const HEADER_POOL_BYTES = 6;
const HEADER_ROOT = 7;
const HEADER_SOURCE_BYTES = 8;

// These field orders are the transfer contract used by parser/mod.rs push_node call sites.
// Extend this table and decodeNode together whenever the native parser starts emitting a new tag.
const NODE_SCHEMAS = [];
for (
  const [tag, schema] of [
    [1, ["Program", ["body", "sourceType"]]],
    [2, ["Identifier", ["name"]]],
    [3, ["PrivateIdentifier", ["name"]]],
    [4, ["Literal", ["raw", "kind"]]],
    [5, ["ExpressionStatement", ["expression"]]],
    [6, ["BlockStatement", ["body"]]],
    [7, ["EmptyStatement", []]],
    [8, ["DebuggerStatement", []]],
    [9, ["WithStatement", ["object", "body"]]],
    [10, ["ReturnStatement", ["argument"]]],
    [11, ["LabeledStatement", ["label", "body"]]],
    [12, ["BreakStatement", ["label"]]],
    [13, ["ContinueStatement", ["label"]]],
    [14, ["IfStatement", ["test", "consequent", "alternate"]]],
    [15, ["SwitchStatement", ["discriminant", "cases"]]],
    [16, ["SwitchCase", ["test", "consequent"]]],
    [17, ["ThrowStatement", ["argument"]]],
    [18, ["TryStatement", ["block", "handler", "finalizer"]]],
    [19, ["CatchClause", ["param", "body"]]],
    [20, ["WhileStatement", ["test", "body"]]],
    [21, ["DoWhileStatement", ["body", "test"]]],
    [22, ["ForStatement", ["init", "test", "update", "body"]]],
    [23, ["ForInStatement", ["left", "right", "body", "await"]]],
    [24, ["ForOfStatement", ["left", "right", "body", "await"]]],
    [
      25,
      ["FunctionDeclaration", ["id", "params", "body", "generator", "async", "returnType", "typeParameters"]],
    ],
    [
      26,
      ["FunctionExpression", ["id", "params", "body", "generator", "async", "returnType", "typeParameters"]],
    ],
    [27, ["ArrowFunctionExpression", ["params", "body", "async", "expression"]]],
    [28, ["VariableDeclaration", ["declarations", "kind"]]],
    [29, ["VariableDeclarator", ["id", "init"]]],
    [30, ["ThisExpression", []]],
    [31, ["ArrayExpression", ["elements"]]],
    [32, ["ObjectExpression", ["properties"]]],
    [33, ["Property", ["key", "value", "kind", "method", "shorthand", "computed"]]],
    [34, ["SequenceExpression", ["expressions"]]],
    [35, ["UnaryExpression", ["operator", "prefix", "argument"]]],
    [36, ["UpdateExpression", ["operator", "prefix", "argument"]]],
    [37, ["BinaryExpression", ["operator", "left", "right"]]],
    [38, ["LogicalExpression", ["operator", "left", "right"]]],
    [39, ["AssignmentExpression", ["operator", "left", "right"]]],
    [40, ["AssignmentPattern", ["left", "right"]]],
    [41, ["ConditionalExpression", ["test", "consequent", "alternate"]]],
    [42, ["NewExpression", ["callee", "arguments"]]],
    [43, ["CallExpression", ["callee", "arguments", "optional"]]],
    [44, ["MemberExpression", ["object", "property", "computed", "optional"]]],
    [45, ["ChainExpression", ["expression"]]],
    [46, ["YieldExpression", ["argument", "delegate"]]],
    [47, ["AwaitExpression", ["argument"]]],
    [48, ["TemplateLiteral", ["quasis", "expressions"]]],
    [49, ["TemplateElement", ["raw", "tail"]]],
    [50, ["TaggedTemplateExpression", ["tag", "quasi"]]],
    [51, ["SpreadElement", ["argument"]]],
    [52, ["RestElement", ["argument"]]],
    [53, ["ArrayPattern", ["elements"]]],
    [54, ["ObjectPattern", ["properties"]]],
    [55, ["MetaProperty", ["meta", "property"]]],
    [56, ["ImportExpression", ["source", "options"]]],
    [57, ["ClassDeclaration", ["id", "superClass", "body"]]],
    [58, ["ClassExpression", ["id", "superClass", "body"]]],
    [59, ["ClassBody", ["body"]]],
    [60, ["MethodDefinition", ["key", "value", "kind", "computed", "static"]]],
    [61, ["PropertyDefinition", ["key", "value", "computed", "static", "typeAnnotation"]]],
    [62, ["StaticBlock", ["block"]]],
    [63, ["ImportDeclaration", ["specifiers", "source", "attributes", "importKind"]]],
    [64, ["ImportSpecifier", ["imported", "local", "importKind"]]],
    [65, ["ImportDefaultSpecifier", ["local"]]],
    [66, ["ImportNamespaceSpecifier", ["local"]]],
    [67, ["ExportNamedDeclaration", ["declaration", "specifiers", "source", "attributes", "exportKind"]]],
    [68, ["ExportDefaultDeclaration", ["declaration"]]],
    [69, ["ExportAllDeclaration", ["source", "exported", "attributes", "exportKind"]]],
    [70, ["ExportSpecifier", ["local", "exported"]]],
    [71, ["Super", []]],
    [72, ["ParenthesizedExpression", ["expression"]]],
    [74, ["ImportExpression", ["source", "options", "phase"]]],
    [256, ["JSXIdentifier", ["name"]]],
    [259, ["JSXElement", ["openingElement", "closingElement", "children"]]],
    [261, ["JSXOpeningElement", ["name", "attributes", "selfClosing"]]],
    [262, ["JSXClosingElement", ["name"]]],
    [265, ["JSXAttribute", ["name", "value"]]],
    [266, ["JSXSpreadAttribute", ["argument"]]],
    [267, ["JSXExpressionContainer", ["expression"]]],
    [268, ["JSXEmptyExpression", []]],
    [269, ["JSXText", ["raw"]]],
    [512, ["TSTypeAnnotation", ["typeAnnotation"]]],
    [513, ["TSTypeReference", ["typeName", "typeArguments"]]],
    [514, ["TSQualifiedName", ["left", "right"]]],
    [515, ["TSUnionType", ["types"]]],
    [516, ["TSIntersectionType", ["types"]]],
    [517, ["TSLiteralType", ["literal"]]],
    [518, ["TSArrayType", ["elementType"]]],
    [519, ["TSTupleType", ["elementTypes"]]],
    [520, ["TSFunctionType", ["typeParameters", "params", "returnType"]]],
    [521, ["TSConditionalType", ["checkType", "extendsType", "trueType", "falseType"]]],
    [522, ["TSMappedType", ["key", "constraint", "nameType", "typeAnnotation", "readonly", "optional"]]],
    [523, ["TSTypeLiteral", ["members"]]],
    [524, ["TSInterfaceDeclaration", ["id", "typeParameters", "extends", "body"]]],
    [525, ["TSTypeAliasDeclaration", ["id", "typeParameters", "typeAnnotation"]]],
    [526, ["TSEnumDeclaration", ["id", "body", "const", "declare"]]],
    [527, ["TSModuleDeclaration", ["id", "body", "declare", "kind"]]],
    [528, ["TSAsExpression", ["expression", "typeAnnotation"]]],
    [529, ["TSSatisfiesExpression", ["expression", "typeAnnotation"]]],
    [530, ["TSNonNullExpression", ["expression"]]],
    [531, ["TSParenthesizedType", ["typeAnnotation"]]],
    [532, ["TSIndexedAccessType", ["objectType", "indexType"]]],
    [533, ["TSTypeOperator", ["operator", "typeAnnotation"]]],
    [534, ["TSTypeParameter", ["name", "const", "in", "out", "constraint", "default"]]],
    [535, ["TSPropertySignature", ["key", "typeAnnotation", "computed", "optional", "readonly"]]],
    [536, ["TSMethodSignature", ["key", "typeParameters", "params", "returnType", "computed", "optional"]]],
    [537, ["TSEnumMember", ["id", "initializer"]]],
    [538, ["TSNamedTupleMember", ["label", "elementType", "optional"]]],
    [539, ["TSInterfaceBody", ["body"]]],
    [540, ["TSModuleBlock", ["body"]]],
    [541, ["TSTypeParameterDeclaration", ["params"]]],
    [542, ["TSTypeParameterInstantiation", ["params"]]],
    [543, ["TSAnyKeyword", []]],
    [544, ["TSBigIntKeyword", []]],
    [545, ["TSBooleanKeyword", []]],
    [546, ["TSIntrinsicKeyword", []]],
    [547, ["TSNeverKeyword", []]],
    [548, ["TSNumberKeyword", []]],
    [549, ["TSObjectKeyword", []]],
    [550, ["TSStringKeyword", []]],
    [551, ["TSSymbolKeyword", []]],
    [552, ["TSThisType", []]],
    [553, ["TSUndefinedKeyword", []]],
    [554, ["TSUnknownKeyword", []]],
    [555, ["TSVoidKeyword", []]],
    [556, ["TSInferType", ["typeParameter"]]],
    [557, ["TSEnumBody", ["members"]]],
    [558, ["TSInterfaceHeritage", ["expression", "typeArguments"]]],
    [559, ["TSNullKeyword", []]],
    [560, ["TSTypeAssertion", ["typeAnnotation", "expression"]]],
    [561, ["TSExportAssignment", ["expression"]]],
    [562, ["TSNamespaceExportDeclaration", ["id"]]],
    [563, ["TSImportEqualsDeclaration", ["id", "moduleReference", "importKind"]]],
    [564, ["TSExternalModuleReference", ["expression"]]],
    [565, ["NewExpression", ["callee", "arguments", "typeArguments"]]],
    [566, ["TSClassImplements", ["expression", "typeArguments"]]],
    [567, ["ClassDeclaration", ["id", "superClass", "body", "implements"]]],
    [568, ["ClassExpression", ["id", "superClass", "body", "implements"]]],
    [569, ["ClassDeclaration", ["id", "superClass", "body", "implements", "typeParameters"]]],
    [570, ["ClassExpression", ["id", "superClass", "body", "implements", "typeParameters"]]],
    [571, ["TSEmptyBodyFunctionExpression", ["id", "params", "generator", "async", "returnType"]]],
  ]
) NODE_SCHEMAS[tag] = schema;

const UNSUPPORTED_NODE_TAGS = [];
for (
  const [tag, name] of [
    [73, "ImportAttribute"],
    [257, "JSXMemberExpression"],
    [258, "JSXNamespacedName"],
    [260, "JSXFragment"],
    [263, "JSXOpeningFragment"],
    [264, "JSXClosingFragment"],
    [270, "JSXSpreadChild"],
  ]
) UNSUPPORTED_NODE_TAGS[tag] = name;

const ASSIGNMENT_OPERATORS = [
  "=",
  "+=",
  "-=",
  "*=",
  "/=",
  "%=",
  "**=",
  "<<=",
  ">>=",
  ">>>=",
  "|=",
  "^=",
  "&=",
  "||=",
  "&&=",
  "??=",
];

const BINARY_OPERATORS = [
  "==",
  "!=",
  "===",
  "!==",
  "<",
  "<=",
  ">",
  ">=",
  "<<",
  ">>",
  ">>>",
  "+",
  "-",
  "*",
  "/",
  "%",
  "**",
  "|",
  "^",
  "&",
  "in",
  "instanceof",
  "||",
  "&&",
  "??",
];

const UNARY_OPERATORS = ["-", "+", "!", "~", "typeof", "void", "delete"];
const UPDATE_OPERATORS = ["++", "--"];
const VARIABLE_KINDS = ["var", "let", "const"];
const PROPERTY_KINDS = ["init", "get", "set"];
const METHOD_KINDS = ["method", "get", "set", "constructor"];
const IMPORT_EXPORT_KINDS = ["value", "type", "typeof"];
const IMPORT_PHASES = ["source", "defer"];
const SOURCE_TYPES = ["script", "module", "commonjs"];
const TS_MODULE_KINDS = ["namespace", "module"];

export function decodeTape(source, tape, options = {}) {
  return decodeTapeInternal(source, tape, options, false);
}

export function decodeTrustedTape(source, tape, options = {}) {
  try {
    return decodeTapeInternal(source, tape, options, true);
  } catch (error) {
    // Recursive materialization is faster for native-validated tapes, but deeply nested valid input must remain decodable.
    if (error instanceof RangeError) return decodeTape(source, tape, options);
    throw error;
  }
}

function decodeTapeInternal(source, tape, options, trusted) {
  if (!(tape instanceof Uint32Array)) {
    throw new TypeError("JetSyntax tape must be a Uint32Array");
  }

  const header = validateHeader(tape);
  if (header.poolBytes > 0 && !HOST_LITTLE_ENDIAN) {
    throw new Error("JetSyntax zero-copy string-pool decoding requires a little-endian host");
  }

  const sourceOffsets = makeSourceOffsets(source, header.sourceBytes, trusted);
  const poolBytes = new Uint8Array(
    tape.buffer,
    tape.byteOffset + header.recordEnd * Uint32Array.BYTES_PER_ELEMENT,
    header.poolBytes,
  );
  const poolDecoder = new TextDecoder("utf-8", { fatal: true });
  const numberBytes = new ArrayBuffer(8);
  const numberView = new DataView(numberBytes);
  const decoded = trusted ? null : new Array(header.recordEnd);

  function sourceSlice(start, end) {
    if (start > end) {
      throw new Error(`invalid JetSyntax source slice ${start}..${end}`);
    }
    return source.slice(sourcePosition(start), sourcePosition(end));
  }

  function sourcePosition(byteOffset) {
    if (byteOffset > header.sourceBytes) {
      throw new Error(`invalid JetSyntax source byte offset ${byteOffset}`);
    }
    if (sourceOffsets === null) return byteOffset;
    if (sourceOffsets.boundaries[byteOffset] !== 1) {
      throw new Error(`invalid JetSyntax source byte offset ${byteOffset}`);
    }
    return sourceOffsets.utf16[byteOffset];
  }

  function poolString(start, length) {
    const end = start + length;
    if (!Number.isSafeInteger(end) || start > end || end > poolBytes.length) {
      throw new Error(`invalid JetSyntax string-pool slice ${start}..${end}`);
    }
    try {
      return poolDecoder.decode(poolBytes.subarray(start, end));
    } catch (error) {
      // Preserve recursive trusted-decoder stack overflows so the outer fallback can switch to the iterative path.
      if (error instanceof RangeError) throw error;
      throw new Error(`invalid UTF-8 in JetSyntax string-pool slice ${start}..${end}`);
    }
  }

  function readDecodedReference(reference, parentOffset) {
    if (reference >= parentOffset || decoded[reference] === undefined) {
      throw new Error(`invalid JetSyntax backward reference ${reference} from ${parentOffset}`);
    }
    requireReferenceMarker(reference);
    return decoded[reference];
  }

  // Native tapes are validated postfix trees, so their references can be materialized without a cache.
  function readTrustedReference(reference, parentOffset) {
    if (reference < HEADER_WORDS || reference >= parentOffset) {
      throw new Error(`invalid JetSyntax backward reference ${reference} from ${parentOffset}`);
    }
    // Native construction already proves marker integrity; keeping this edge compact raises the recursion ceiling.
    return decodeTrustedValue(reference);
  }

  function requireReferenceMarker(reference) {
    if (header.referenceMarkers && (tape[reference] & REFERENCE_MARKER) === 0) {
      throw new Error(`missing JetSyntax reference marker at word ${reference}`);
    }
  }

  function valueHeader(offset) {
    const record = tape[offset];
    return header.referenceMarkers ? (record & ~REFERENCE_MARKER) >>> 0 : record;
  }

  const readReference = trusted ? readTrustedReference : readDecodedReference;

  function decodeTrustedValue(offset) {
    requireWords(offset, 1, header.recordEnd);
    const record = valueHeader(offset);
    const kind = (record & KIND_MASK) >>> KIND_SHIFT;
    switch (kind) {
      case KIND_NODE: {
        requireWords(offset, 5, header.recordEnd);
        const size = 5 + tape[offset + 4];
        if (tape[offset + 1] !== size) {
          throw new Error(`invalid JetSyntax node length at word ${offset}`);
        }
        requireWords(offset, size, header.recordEnd);
        return decodeNode(offset, record);
      }
      case KIND_LIST: {
        requireWords(offset, 3, header.recordEnd);
        const count = tape[offset + 2];
        const size = 3 + count;
        if (record !== KIND_LIST << KIND_SHIFT || tape[offset + 1] !== size) {
          throw new Error(`invalid JetSyntax list record at word ${offset}`);
        }
        requireWords(offset, size, header.recordEnd);
        const value = new Array(count);
        for (let index = 0; index < count; index++) {
          value[index] = readTrustedReference(tape[offset + 3 + index], offset);
        }
        return value;
      }
      case KIND_NULL:
        if (record !== (KIND_NULL << KIND_SHIFT) >>> 0) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        return null;
      case KIND_BOOL:
        if ((record & ~1) !== KIND_BOOL << KIND_SHIFT) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        return (record & 1) !== 0;
      case KIND_INLINE_U32:
        return record & header.inlineU32Mask;
      case KIND_U32:
        if (record !== (KIND_U32 << KIND_SHIFT) >>> 0) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        requireWords(offset, 2, header.recordEnd);
        return tape[offset + 1];
      case KIND_F64:
        if (record !== (KIND_F64 << KIND_SHIFT) >>> 0) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        requireWords(offset, 3, header.recordEnd);
        numberView.setUint32(0, tape[offset + 1], true);
        numberView.setUint32(4, tape[offset + 2], true);
        return numberView.getFloat64(0, true);
      case KIND_SOURCE_SLICE:
        if (record !== (KIND_SOURCE_SLICE << KIND_SHIFT) >>> 0) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        requireWords(offset, 3, header.recordEnd);
        return sourceSlice(tape[offset + 1], tape[offset + 2]);
      case KIND_POOL_STRING:
        if (record !== (KIND_POOL_STRING << KIND_SHIFT) >>> 0) {
          throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
        }
        requireWords(offset, 3, header.recordEnd);
        return poolString(tape[offset + 1], tape[offset + 2]);
      default:
        throw new Error(`unknown JetSyntax tape value kind ${kind} at word ${offset}`);
    }
  }

  function decodeTrustedRoot() {
    const root = header.root;
    if (root < HEADER_WORDS || root >= header.recordEnd) {
      throw new Error(`invalid JetSyntax tape root offset ${root}`);
    }
    if (header.referenceMarkers && (tape[root] & REFERENCE_MARKER) !== 0) {
      throw new Error("invalid JetSyntax tape: root carries a reference marker");
    }
    const record = valueHeader(root);
    if ((record >>> KIND_SHIFT) !== KIND_NODE) {
      throw new Error("invalid JetSyntax tape: root record is not a node");
    }
    requireWords(root, 5, header.recordEnd);
    const size = 5 + tape[root + 4];
    if (tape[root + 1] !== size || root + size !== header.recordEnd) {
      throw new Error(`invalid JetSyntax tape root offset ${root}`);
    }
    requireWords(root, size, header.recordEnd);
    return decodeNode(root, record);
  }

  function decodeRecords() {
    let offset = HEADER_WORDS;
    let last = 0;
    while (offset < header.recordEnd) {
      last = offset;
      const record = valueHeader(offset);
      const kind = (record & KIND_MASK) >>> KIND_SHIFT;
      let size;
      let value;
      switch (kind) {
        case KIND_NODE:
          requireWords(offset, 5, header.recordEnd);
          size = 5 + tape[offset + 4];
          if (tape[offset + 1] !== size) {
            throw new Error(`invalid JetSyntax node length at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          value = decodeNode(offset, record);
          break;
        case KIND_LIST: {
          requireWords(offset, 3, header.recordEnd);
          const count = tape[offset + 2];
          size = 3 + count;
          if (record !== KIND_LIST << KIND_SHIFT || tape[offset + 1] !== size) {
            throw new Error(`invalid JetSyntax list record at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          value = new Array(count);
          for (let index = 0; index < count; index++) {
            value[index] = readReference(tape[offset + 3 + index], offset);
          }
          break;
        }
        case KIND_NULL:
          size = 1;
          if (record !== (KIND_NULL << KIND_SHIFT) >>> 0) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          value = null;
          break;
        case KIND_BOOL:
          size = 1;
          if ((record & ~1) !== KIND_BOOL << KIND_SHIFT) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          value = (record & 1) !== 0;
          break;
        case KIND_INLINE_U32:
          size = 1;
          value = record & header.inlineU32Mask;
          break;
        case KIND_U32:
          size = 2;
          if (record !== (KIND_U32 << KIND_SHIFT) >>> 0) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          value = tape[offset + 1];
          break;
        case KIND_F64:
          size = 3;
          if (record !== (KIND_F64 << KIND_SHIFT) >>> 0) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          numberView.setUint32(0, tape[offset + 1], true);
          numberView.setUint32(4, tape[offset + 2], true);
          value = numberView.getFloat64(0, true);
          break;
        case KIND_SOURCE_SLICE:
          size = 3;
          if (record !== (KIND_SOURCE_SLICE << KIND_SHIFT) >>> 0) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          value = sourceSlice(tape[offset + 1], tape[offset + 2]);
          break;
        case KIND_POOL_STRING:
          size = 3;
          if (record !== (KIND_POOL_STRING << KIND_SHIFT) >>> 0) {
            throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
          }
          requireWords(offset, size, header.recordEnd);
          value = poolString(tape[offset + 1], tape[offset + 2]);
          break;
        default:
          throw new Error(`unknown JetSyntax tape value kind ${kind} at word ${offset}`);
      }
      decoded[offset] = value;
      offset += size;
    }
    if (offset !== header.recordEnd) {
      throw new Error("JetSyntax record section ends inside a record");
    }
    if (header.root !== last || decoded[header.root] === undefined) {
      throw new Error(`invalid JetSyntax tape root offset ${header.root}`);
    }
    if ((tape[header.root] >>> KIND_SHIFT) !== KIND_NODE) {
      throw new Error("invalid JetSyntax tape: root record is not a node");
    }
    if (header.referenceMarkers && (tape[header.root] & REFERENCE_MARKER) !== 0) {
      throw new Error("invalid JetSyntax tape: root carries a reference marker");
    }
    return decoded[header.root];
  }

  function decodeNode(offset, record) {
    const tag = record & NODE_TAG_MASK;
    const schema = NODE_SCHEMAS[tag];
    if (!schema) {
      const unsupported = UNSUPPORTED_NODE_TAGS[tag];
      if (unsupported) {
        throw new Error(`unsupported JetSyntax node tag ${tag} (${unsupported})`);
      }
      throw new Error(`unknown JetSyntax node tag ${tag}`);
    }

    const fieldCount = tape[offset + 4];
    // TypeScript return and type-parameter annotations extend the five-field JavaScript shape.
    const functionNode = tag === 25 || tag === 26;
    const validFieldCount = tag === 2
      ? fieldCount === 1 || fieldCount === 3
      : functionNode
      ? fieldCount === 5 || fieldCount === 6 || fieldCount === 7
      : fieldCount === schema[1].length;
    if (!validFieldCount) {
      const expected = tag === 2 ? "1 or 3" : functionNode ? "5, 6, or 7" : schema[1].length;
      throw new Error(
        `invalid ${schema[0]} field count ${fieldCount}; expected ${expected}`,
      );
    }
    if ((record & ~(KIND_MASK | NODE_FLAGS_MASK | NODE_TAG_MASK)) !== 0) {
      throw new Error(`invalid reserved bits in JetSyntax node tag ${tag}`);
    }

    const start = sourcePosition(tape[offset + 2]);
    const end = sourcePosition(tape[offset + 3]);
    const fields = new Array(fieldCount);
    for (let index = 0; index < fieldCount; index++) {
      fields[index] = readReference(tape[offset + 5 + index], offset);
    }
    const node = { type: schema[0], start, end };
    if (options.range) node.range = [start, end];

    switch (tag) {
      case 1:
        node.body = array(fields[0], tag);
        node.sourceType = enumValue(SOURCE_TYPES, fields[1], tag);
        return node;
      case 2:
        node.name = string(fields[0], tag);
        if (fieldCount === 3) {
          node.typeAnnotation = fields[1];
          node.optional = boolean(fields[2], tag);
        }
        return node;
      case 3:
        node.name = string(fields[0], tag);
        return node;
      case 4:
        decodeLiteral(node, string(fields[0], tag), integer(fields[1], tag));
        return node;
      case 5:
        node.expression = fields[0];
        return node;
      case 6:
        node.body = array(fields[0], tag);
        return node;
      case 7:
      case 8:
      case 30:
      case 71:
        return node;
      case 9:
        node.object = fields[0];
        node.body = fields[1];
        return node;
      case 10:
        node.argument = fields[0];
        return node;
      case 11:
        node.label = fields[0];
        node.body = fields[1];
        return node;
      case 12:
      case 13:
        node.label = fields[0];
        return node;
      case 14:
        node.test = fields[0];
        node.consequent = fields[1];
        node.alternate = fields[2];
        return node;
      case 15:
        node.discriminant = fields[0];
        node.cases = array(fields[1], tag);
        return node;
      case 16:
        node.test = fields[0];
        node.consequent = array(fields[1], tag);
        return node;
      case 17:
        node.argument = fields[0];
        return node;
      case 18:
        node.block = fields[0];
        node.handler = fields[1];
        node.finalizer = fields[2];
        return node;
      case 19:
        node.param = fields[0];
        node.body = fields[1];
        return node;
      case 20:
        node.test = fields[0];
        node.body = fields[1];
        return node;
      case 21:
        node.body = fields[0];
        node.test = fields[1];
        return node;
      case 22:
        node.init = fields[0];
        node.test = fields[1];
        node.update = fields[2];
        node.body = fields[3];
        return node;
      case 23:
        node.left = fields[0];
        node.right = fields[1];
        node.body = fields[2];
        return node;
      case 24:
        node.left = fields[0];
        node.right = fields[1];
        node.body = fields[2];
        node.await = boolean(fields[3], tag);
        return node;
      case 25:
      case 26:
        node.id = fields[0];
        node.params = array(fields[1], tag);
        node.body = fields[2];
        node.generator = boolean(fields[3], tag);
        node.async = boolean(fields[4], tag);
        if (fieldCount >= 6 && fields[5] !== null) node.returnType = fields[5];
        if (fieldCount === 7) node.typeParameters = fields[6];
        return node;
      case 27:
        node.id = null;
        node.params = array(fields[0], tag);
        node.body = fields[1];
        node.generator = false;
        node.async = boolean(fields[2], tag);
        node.expression = boolean(fields[3], tag);
        return node;
      case 28:
        node.declarations = array(fields[0], tag);
        node.kind = enumValue(VARIABLE_KINDS, fields[1], tag);
        return node;
      case 29:
        node.id = fields[0];
        node.init = fields[1];
        return node;
      case 31:
        node.elements = array(fields[0], tag);
        return node;
      case 32:
        node.properties = array(fields[0], tag);
        return node;
      case 33:
        node.key = fields[0];
        node.value = fields[1];
        node.kind = enumValue(PROPERTY_KINDS, fields[2], tag);
        node.method = boolean(fields[3], tag);
        node.shorthand = boolean(fields[4], tag);
        node.computed = boolean(fields[5], tag);
        return node;
      case 34:
        node.expressions = array(fields[0], tag);
        return node;
      case 35:
        node.operator = enumValue(UNARY_OPERATORS, fields[0], tag);
        node.prefix = boolean(fields[1], tag);
        node.argument = fields[2];
        return node;
      case 36:
        node.operator = enumValue(UPDATE_OPERATORS, fields[0], tag);
        node.prefix = boolean(fields[1], tag);
        node.argument = fields[2];
        return node;
      case 37:
      case 38:
        node.operator = enumValue(BINARY_OPERATORS, fields[0], tag);
        node.left = fields[1];
        node.right = fields[2];
        return node;
      case 39:
        node.operator = enumValue(ASSIGNMENT_OPERATORS, fields[0], tag);
        node.left = fields[1];
        node.right = fields[2];
        return node;
      case 40:
        node.left = fields[0];
        node.right = fields[1];
        return node;
      case 41:
        node.test = fields[0];
        node.consequent = fields[1];
        node.alternate = fields[2];
        return node;
      case 42:
        node.callee = fields[0];
        node.arguments = array(fields[1], tag);
        return node;
      case 43:
        node.callee = fields[0];
        node.arguments = array(fields[1], tag);
        node.optional = boolean(fields[2], tag);
        return node;
      case 44:
        node.object = fields[0];
        node.property = fields[1];
        node.computed = boolean(fields[2], tag);
        node.optional = boolean(fields[3], tag);
        return node;
      case 45:
        node.expression = fields[0];
        return node;
      case 46:
        node.argument = fields[0];
        node.delegate = boolean(fields[1], tag);
        return node;
      case 47:
        node.argument = fields[0];
        return node;
      case 48:
        node.quasis = array(fields[0], tag);
        node.expressions = array(fields[1], tag);
        return node;
      case 49: {
        const raw = templateElementRaw(string(fields[0], tag));
        node.value = { raw, cooked: decodeQuotedString(`\"${raw}\"`) };
        node.tail = boolean(fields[1], tag);
        return node;
      }
      case 50:
        node.tag = fields[0];
        node.quasi = fields[1];
        return node;
      case 51:
      case 52:
        node.argument = fields[0];
        return node;
      case 53:
        node.elements = patternItems(fields[0], "elements", tag);
        return node;
      case 54:
        node.properties = patternItems(fields[0], "properties", tag);
        return node;
      case 55:
        node.meta = fields[0];
        node.property = fields[1];
        return node;
      case 56:
        node.source = fields[0];
        node.options = fields[1];
        return node;
      case 57:
      case 58:
        node.id = fields[0];
        node.superClass = fields[1];
        node.body = fields[2];
        return node;
      case 59:
        node.body = array(fields[0], tag);
        return node;
      case 60:
        node.key = fields[0];
        node.value = fields[1];
        node.kind = enumValue(METHOD_KINDS, fields[2], tag);
        node.computed = boolean(fields[3], tag);
        node.static = boolean(fields[4], tag);
        return node;
      case 61:
        node.key = fields[0];
        node.value = fields[1];
        node.computed = boolean(fields[2], tag);
        node.static = boolean(fields[3], tag);
        node.typeAnnotation = fields[4];
        return node;
      case 62: {
        const block = fields[0];
        if (block?.type !== "BlockStatement") {
          throw new Error("JetSyntax StaticBlock expected a BlockStatement field");
        }
        node.body = block.body;
        return node;
      }
      case 63:
        node.specifiers = array(fields[0], tag);
        node.source = fields[1];
        node.attributes = array(fields[2], tag);
        node.importKind = enumValue(IMPORT_EXPORT_KINDS, fields[3], tag);
        return node;
      case 64:
        node.imported = fields[0];
        node.local = fields[1];
        node.importKind = enumValue(IMPORT_EXPORT_KINDS, fields[2], tag);
        return node;
      case 65:
      case 66:
        node.local = fields[0];
        return node;
      case 67:
        node.declaration = fields[0];
        node.specifiers = array(fields[1], tag);
        node.source = fields[2];
        node.attributes = array(fields[3], tag);
        node.exportKind = enumValue(IMPORT_EXPORT_KINDS, fields[4], tag);
        return node;
      case 68:
        node.declaration = fields[0];
        return node;
      case 69:
        node.source = fields[0];
        node.exported = fields[1];
        node.attributes = array(fields[2], tag);
        node.exportKind = enumValue(IMPORT_EXPORT_KINDS, fields[3], tag);
        return node;
      case 70:
        node.local = fields[0];
        node.exported = fields[1];
        return node;
      case 72:
        node.expression = fields[0];
        return node;
      case 256:
        node.name = string(fields[0], tag);
        return node;
      case 259:
        node.openingElement = fields[0];
        node.closingElement = fields[1];
        node.children = array(fields[2], tag);
        return node;
      case 261:
        node.name = fields[0];
        node.attributes = array(fields[1], tag);
        node.selfClosing = boolean(fields[2], tag);
        return node;
      case 262:
        node.name = fields[0];
        return node;
      case 265:
        node.name = fields[0];
        node.value = fields[1];
        return node;
      case 266:
        node.argument = fields[0];
        return node;
      case 267:
        node.expression = fields[0];
        return node;
      case 268:
        return node;
      case 269: {
        const raw = string(fields[0], tag);
        node.value = raw;
        node.raw = raw;
        return node;
      }
      case 512:
        node.typeAnnotation = fields[0];
        return node;
      case 513:
        node.typeName = fields[0];
        node.typeArguments = fields[1];
        return node;
      case 514:
        node.left = fields[0];
        node.right = fields[1];
        return node;
      case 515:
      case 516:
        node.types = array(fields[0], tag);
        return node;
      case 517:
        node.literal = fields[0];
        return node;
      case 518:
        node.elementType = fields[0];
        return node;
      case 519:
        node.elementTypes = array(fields[0], tag);
        return node;
      case 520:
        node.typeParameters = fields[0];
        node.params = array(fields[1], tag);
        node.returnType = fields[2];
        return node;
      case 521:
        node.checkType = fields[0];
        node.extendsType = fields[1];
        node.trueType = fields[2];
        node.falseType = fields[3];
        return node;
      case 522:
        node.key = fields[0];
        node.constraint = fields[1];
        node.nameType = fields[2];
        node.typeAnnotation = fields[3];
        if (fields[4] !== null) node.readonly = boolean(fields[4], tag);
        node.optional = boolean(fields[5], tag);
        return node;
      case 523:
        node.members = array(fields[0], tag);
        return node;
      case 524:
        node.id = fields[0];
        node.typeParameters = fields[1];
        node.extends = array(fields[2], tag);
        node.body = fields[3];
        node.declare = false;
        return node;
      case 525:
        node.id = fields[0];
        node.typeParameters = fields[1];
        node.typeAnnotation = fields[2];
        node.declare = false;
        return node;
      case 526:
        node.id = fields[0];
        node.body = fields[1];
        node.const = boolean(fields[2], tag);
        node.declare = boolean(fields[3], tag);
        return node;
      case 527:
        node.id = fields[0];
        node.body = fields[1];
        node.declare = boolean(fields[2], tag);
        node.kind = enumValue(TS_MODULE_KINDS, fields[3], tag);
        return node;
      case 528:
      case 529:
        node.expression = fields[0];
        node.typeAnnotation = fields[1];
        return node;
      case 530:
        node.expression = fields[0];
        return node;
      case 560:
        node.typeAnnotation = fields[0];
        node.expression = fields[1];
        return node;
      case 561:
        node.expression = fields[0];
        return node;
      case 562:
        node.id = fields[0];
        return node;
      case 563:
        node.id = fields[0];
        node.moduleReference = fields[1];
        node.importKind = enumValue(IMPORT_EXPORT_KINDS, fields[2], tag);
        return node;
      case 564:
        node.expression = fields[0];
        return node;
      case 565:
        node.callee = fields[0];
        node.arguments = array(fields[1], tag);
        node.typeArguments = fields[2];
        return node;
      case 566:
        node.expression = fields[0];
        node.typeArguments = fields[1];
        return node;
      case 567:
      case 568:
        node.id = fields[0];
        node.superClass = fields[1];
        node.body = fields[2];
        node.implements = array(fields[3], tag);
        return node;
      case 569:
      case 570:
        node.id = fields[0];
        node.superClass = fields[1];
        node.body = fields[2];
        if (fields[3] !== null) node.implements = array(fields[3], tag);
        node.typeParameters = fields[4];
        return node;
      case 571:
        node.id = fields[0];
        node.params = array(fields[1], tag);
        node.body = null;
        node.generator = boolean(fields[2], tag);
        node.async = boolean(fields[3], tag);
        node.expression = false;
        node.declare = false;
        if (fields[4] !== null) node.returnType = fields[4];
        return node;
      case 531:
        node.typeAnnotation = fields[0];
        return node;
      case 532:
        node.objectType = fields[0];
        node.indexType = fields[1];
        return node;
      case 533:
        node.operator = string(fields[0], tag);
        node.typeAnnotation = fields[1];
        return node;
      case 534:
        node.name = fields[0];
        node.const = boolean(fields[1], tag);
        node.in = boolean(fields[2], tag);
        node.out = boolean(fields[3], tag);
        node.constraint = fields[4];
        node.default = fields[5];
        return node;
      case 535:
        node.key = fields[0];
        node.typeAnnotation = fields[1];
        node.computed = boolean(fields[2], tag);
        node.optional = boolean(fields[3], tag);
        node.readonly = boolean(fields[4], tag);
        node.accessibility = null;
        node.static = false;
        return node;
      case 536:
        node.key = fields[0];
        node.typeParameters = fields[1];
        node.params = array(fields[2], tag);
        node.returnType = fields[3];
        node.computed = boolean(fields[4], tag);
        node.optional = boolean(fields[5], tag);
        node.kind = "method";
        node.accessibility = null;
        node.readonly = false;
        node.static = false;
        return node;
      case 537:
        node.id = fields[0];
        node.initializer = fields[1];
        return node;
      case 538:
        node.label = fields[0];
        node.elementType = fields[1];
        node.optional = boolean(fields[2], tag);
        return node;
      case 539:
      case 540:
        node.body = array(fields[0], tag);
        return node;
      case 541:
      case 542:
        node.params = array(fields[0], tag);
        return node;
      case 543:
      case 544:
      case 545:
      case 546:
      case 547:
      case 548:
      case 549:
      case 550:
      case 551:
      case 552:
      case 553:
      case 554:
      case 555:
      case 559:
        return node;
      case 556:
        node.typeParameter = fields[0];
        return node;
      case 557:
        node.members = array(fields[0], tag);
        return node;
      case 558:
        node.expression = fields[0];
        node.typeArguments = fields[1];
        return node;
      case 74:
        node.source = fields[0];
        node.options = fields[1];
        node.phase = enumValue(IMPORT_PHASES, fields[2], tag);
        return node;
      default:
        throw new Error(`missing ESTree decoder for JetSyntax node tag ${tag}`);
    }
  }

  return trusted ? decodeTrustedRoot() : decodeRecords();
}

function validateHeader(tape) {
  if (tape.length < HEADER_WORDS) throw new Error("truncated JetSyntax tape header");
  if (tape[0] !== MAGIC) throw new Error("invalid JetSyntax tape magic");
  if (tape[1] !== FORMAT_VERSION) {
    throw new Error(`unsupported JetSyntax tape version ${tape[1]}`);
  }
  if (tape[2] !== HEADER_WORDS) throw new Error("invalid JetSyntax tape header size");
  const flags = tape[HEADER_FLAGS];
  if (flags !== WIRE_FLAGS && flags !== PARSER_WIRE_FLAGS) {
    throw new Error("unsupported JetSyntax tape flags");
  }
  if (tape[HEADER_TOTAL_WORDS] !== tape.length) {
    throw new Error("invalid JetSyntax tape total word count");
  }
  const recordEnd = tape[HEADER_RECORD_END];
  const poolBytes = tape[HEADER_POOL_BYTES];
  if (
    recordEnd <= HEADER_WORDS
    || recordEnd + Math.ceil(poolBytes / Uint32Array.BYTES_PER_ELEMENT) !== tape.length
  ) {
    throw new Error("invalid JetSyntax record/string-pool bounds");
  }
  return {
    inlineU32Mask: flags === PARSER_WIRE_FLAGS ? MARKED_INLINE_U32_MASK : INLINE_U32_MASK,
    referenceMarkers: flags === PARSER_WIRE_FLAGS,
    recordEnd,
    poolBytes,
    root: tape[HEADER_ROOT],
    sourceBytes: tape[HEADER_SOURCE_BYTES],
  };
}

function requireWords(offset, size, recordEnd) {
  if (!Number.isSafeInteger(size) || size <= 0 || offset + size > recordEnd) {
    throw new Error(`truncated JetSyntax record at word ${offset}`);
  }
}

function makeSourceOffsets(source, byteLength, trusted) {
  // Native output was built from this source; equal UTF-8 and UTF-16 lengths therefore prove it is ASCII.
  if (trusted && source.length === byteLength) return null;

  const encodedByteLength = new TextEncoder().encode(source).length;
  if (encodedByteLength !== byteLength) {
    throw new Error("source UTF-8 length does not match JetSyntax input");
  }
  if (source.length === byteLength) return null;
  const utf16 = new Uint32Array(byteLength + 1);
  const boundaries = new Uint8Array(byteLength + 1);
  let byteOffset = 0;
  let utf16Offset = 0;
  boundaries[0] = 1;
  while (utf16Offset < source.length) {
    const codePoint = source.codePointAt(utf16Offset);
    const encodedLength = codePoint <= 0x7F ? 1 : codePoint <= 0x7FF ? 2 : codePoint <= 0xFFFF ? 3 : 4;
    byteOffset += encodedLength;
    utf16Offset += codePoint > 0xFFFF ? 2 : 1;
    if (byteOffset > byteLength) throw new Error("source UTF-8 length does not match JetSyntax input");
    utf16[byteOffset] = utf16Offset;
    boundaries[byteOffset] = 1;
  }
  if (byteOffset !== byteLength) throw new Error("source UTF-8 length does not match JetSyntax input");
  return { utf16, boundaries };
}

function decodeLiteral(node, raw, kind) {
  switch (kind) {
    case 0:
      node.value = Number(raw.replaceAll("_", ""));
      break;
    case 1:
      node.value = decodeQuotedString(raw);
      break;
    case 2:
      node.value = raw === "true";
      break;
    case 3:
      node.value = null;
      break;
    case 4: {
      const bigint = raw.replaceAll("_", "").replace(/n$/u, "");
      node.value = BigInt(bigint);
      node.bigint = bigint;
      break;
    }
    case 5:
      node.value = decodeQuotedString(`\"${raw.slice(1, -1)}\"`);
      break;
    case 6: {
      const slash = lastRegexpSlash(raw);
      const pattern = slash === -1 ? raw.slice(1) : raw.slice(1, slash);
      const flags = slash === -1 ? "" : raw.slice(slash + 1);
      let value = null;
      try {
        value = new RegExp(pattern, flags);
      } catch {
        // Future regular-expression syntax may be valid to JetSyntax but unsupported by this host.
      }
      node.value = value;
      node.regex = { pattern, flags };
      break;
    }
    default:
      throw new Error(`unsupported JetSyntax literal kind ${kind}`);
  }
  node.raw = raw;
}

function decodeQuotedString(raw) {
  if (raw.length < 2) return raw;
  let value = "";
  for (let index = 1; index < raw.length - 1; index++) {
    const character = raw[index];
    if (character !== "\\") {
      value += character;
      continue;
    }
    index++;
    const escaped = raw[index];
    const simple = { b: "\b", f: "\f", n: "\n", r: "\r", t: "\t", v: "\v", 0: "\0" }[escaped];
    if (simple !== undefined) {
      value += simple;
    } else if (escaped === "x") {
      value += String.fromCodePoint(Number.parseInt(raw.slice(index + 1, index + 3), 16));
      index += 2;
    } else if (escaped === "u" && raw[index + 1] === "{") {
      const close = raw.indexOf("}", index + 2);
      if (close === -1) {
        value += raw.slice(index, -1);
        break;
      }
      value += String.fromCodePoint(Number.parseInt(raw.slice(index + 2, close), 16));
      index = close;
    } else if (escaped === "u") {
      value += String.fromCodePoint(Number.parseInt(raw.slice(index + 1, index + 5), 16));
      index += 4;
    } else if (escaped === "\r" && raw[index + 1] === "\n") {
      index++;
    } else if (escaped !== "\n" && escaped !== "\r") {
      value += escaped;
    }
  }
  return value;
}

function lastRegexpSlash(raw) {
  for (let index = raw.length - 1; index > 0; index--) {
    if (raw[index] !== "/") continue;
    let backslashes = 0;
    for (let cursor = index - 1; cursor >= 0 && raw[cursor] === "\\"; cursor--) backslashes++;
    if (backslashes % 2 === 0) return index;
  }
  return -1;
}

function templateElementRaw(tokenRaw) {
  let start = 0;
  let end = tokenRaw.length;
  if (tokenRaw.startsWith("`") || tokenRaw.startsWith("}")) start++;
  if (tokenRaw.endsWith("${")) end -= 2;
  else if (tokenRaw.endsWith("`")) end--;
  return tokenRaw.slice(start, end);
}

function array(value, tag) {
  if (!Array.isArray(value)) throw new Error(`JetSyntax node tag ${tag} expected a list field`);
  return value;
}

function string(value, tag) {
  if (typeof value !== "string") throw new Error(`JetSyntax node tag ${tag} expected a string field`);
  return value;
}

function integer(value, tag) {
  if (!Number.isInteger(value) || value < 0) {
    throw new Error(`JetSyntax node tag ${tag} expected an unsigned integer field`);
  }
  return value;
}

function boolean(value, tag) {
  if (typeof value !== "boolean") throw new Error(`JetSyntax node tag ${tag} expected a Boolean field`);
  return value;
}

function enumValue(values, index, tag) {
  integer(index, tag);
  const value = values[index];
  if (value === undefined) throw new Error(`JetSyntax node tag ${tag} has invalid enum value ${index}`);
  return value;
}

function patternItems(value, property, tag) {
  if (Array.isArray(value)) return value;
  const items = value?.[property];
  if (Array.isArray(items)) return items;
  if (value?.type === "ParenthesizedExpression") return patternItems(value.expression, property, tag);
  if (value && typeof value === "object" && typeof value.type === "string") return [value];
  throw new Error(`JetSyntax node tag ${tag} expected a list or recoverable expression field`);
}
