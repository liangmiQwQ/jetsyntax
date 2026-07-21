const MAGIC = 0x4A53_5450;
const FORMAT_VERSION = 1;
const HEADER_WORDS = 12;

const KIND_SHIFT = 28;
const KIND_MASK = 0xF000_0000;
const NODE_FLAGS_MASK = 0x00FF_0000;
const NODE_TAG_MASK = 0x0000_FFFF;
const INLINE_U32_MASK = 0x0FFF_FFFF;

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
const HEADER_RECORD_END = 5;
const HEADER_POOL_BYTES = 6;
const HEADER_ROOT = 7;

// These field orders are the transfer contract used by parser/mod.rs push_node call sites.
// Extend this table and decodeNode together whenever the native parser starts emitting a new tag.
const NODE_SCHEMAS = new Map([
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
  [25, ["FunctionDeclaration", ["id", "params", "body", "generator", "async"]]],
  [26, ["FunctionExpression", ["id", "params", "body", "generator", "async"]]],
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
  [513, ["TSTypeReference", ["typeName", "typeParameters"]]],
  [514, ["TSQualifiedName", ["left", "right"]]],
]);

const UNSUPPORTED_NODE_TAGS = new Map([
  [73, "ImportAttribute"],
  [257, "JSXMemberExpression"],
  [258, "JSXNamespacedName"],
  [260, "JSXFragment"],
  [263, "JSXOpeningFragment"],
  [264, "JSXClosingFragment"],
  [270, "JSXSpreadChild"],
  [515, "TSUnionType"],
  [516, "TSIntersectionType"],
  [517, "TSLiteralType"],
  [518, "TSArrayType"],
  [519, "TSTupleType"],
  [520, "TSFunctionType"],
  [521, "TSConditionalType"],
  [522, "TSMappedType"],
  [523, "TSTypeLiteral"],
  [524, "TSInterfaceDeclaration"],
  [525, "TSTypeAliasDeclaration"],
  [526, "TSEnumDeclaration"],
  [527, "TSModuleDeclaration"],
  [528, "TSAsExpression"],
  [529, "TSSatisfiesExpression"],
  [530, "TSNonNullExpression"],
]);

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
const SOURCE_TYPES = ["script", "module", "commonjs"];

export function decodeTape(source, tape, options = {}) {
  if (!(tape instanceof Uint32Array)) {
    throw new TypeError("JetSyntax tape must be a Uint32Array");
  }

  const header = validateHeader(tape);
  if (header.poolBytes > 0 && !HOST_LITTLE_ENDIAN) {
    throw new Error("JetSyntax zero-copy string-pool decoding requires a little-endian host");
  }
  const recordStarts = indexRecords(tape, header.recordEnd);
  if (!recordStarts.has(header.root) || header.root !== recordStarts.last) {
    throw new Error(`invalid JetSyntax tape root offset ${header.root}`);
  }
  if ((tape[header.root] >>> KIND_SHIFT) !== KIND_NODE) {
    throw new Error("invalid JetSyntax tape: root record is not a node");
  }

  const sourceBytes = new TextEncoder().encode(source);
  const sourceOffsets = makeSourceOffsets(source, sourceBytes.length);
  const poolBytes = new Uint8Array(
    tape.buffer,
    tape.byteOffset + header.recordEnd * Uint32Array.BYTES_PER_ELEMENT,
    header.poolBytes,
  );
  const poolDecoder = new TextDecoder("utf-8", { fatal: true });
  const numberBytes = new ArrayBuffer(8);
  const numberView = new DataView(numberBytes);
  const decoded = new Map();

  function sourceSlice(start, end) {
    if (start > end) {
      throw new Error(`invalid JetSyntax source slice ${start}..${end}`);
    }
    return source.slice(sourcePosition(start), sourcePosition(end));
  }

  function sourcePosition(byteOffset) {
    if (byteOffset > sourceBytes.length || sourceOffsets.boundaries[byteOffset] !== 1) {
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
    } catch {
      throw new Error(`invalid UTF-8 in JetSyntax string-pool slice ${start}..${end}`);
    }
  }

  function readReference(reference, parentOffset) {
    if (reference >= parentOffset || !recordStarts.has(reference)) {
      throw new Error(`invalid JetSyntax backward reference ${reference} from ${parentOffset}`);
    }
    return readValue(reference);
  }

  function readValue(offset) {
    if (!recordStarts.has(offset)) {
      throw new Error(`invalid JetSyntax record offset ${offset}`);
    }
    if (decoded.has(offset)) return decoded.get(offset);

    const record = tape[offset];
    const kind = (record & KIND_MASK) >>> KIND_SHIFT;
    let value;
    switch (kind) {
      case KIND_NODE:
        value = decodeNode(offset, record);
        break;
      case KIND_LIST: {
        const count = tape[offset + 2];
        value = new Array(count);
        for (let index = 0; index < count; index++) {
          value[index] = readReference(tape[offset + 3 + index], offset);
        }
        break;
      }
      case KIND_NULL:
        value = null;
        break;
      case KIND_BOOL:
        value = (record & 1) !== 0;
        break;
      case KIND_INLINE_U32:
        value = record & INLINE_U32_MASK;
        break;
      case KIND_U32:
        value = tape[offset + 1];
        break;
      case KIND_F64:
        numberView.setUint32(0, tape[offset + 1], true);
        numberView.setUint32(4, tape[offset + 2], true);
        value = numberView.getFloat64(0, true);
        break;
      case KIND_SOURCE_SLICE:
        value = sourceSlice(tape[offset + 1], tape[offset + 2]);
        break;
      case KIND_POOL_STRING:
        value = poolString(tape[offset + 1], tape[offset + 2]);
        break;
      default:
        throw new Error(`unknown JetSyntax tape value kind ${kind} at word ${offset}`);
    }
    decoded.set(offset, value);
    return value;
  }

  function decodeNode(offset, record) {
    const tag = record & NODE_TAG_MASK;
    const schema = NODE_SCHEMAS.get(tag);
    if (!schema) {
      const unsupported = UNSUPPORTED_NODE_TAGS.get(tag);
      if (unsupported) {
        throw new Error(`unsupported JetSyntax node tag ${tag} (${unsupported})`);
      }
      throw new Error(`unknown JetSyntax node tag ${tag}`);
    }

    const fieldCount = tape[offset + 4];
    const validFieldCount = tag === 2 ? fieldCount === 1 || fieldCount === 3 : fieldCount === schema[1].length;
    if (!validFieldCount) {
      const expected = tag === 2 ? "1 or 3" : schema[1].length;
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
    const base = { type: schema[0], start, end };
    if (options.range) base.range = [start, end];

    switch (tag) {
      case 1:
        return { ...base, body: array(fields[0], tag), sourceType: enumValue(SOURCE_TYPES, fields[1], tag) };
      case 2:
        return fieldCount === 1
          ? { ...base, name: string(fields[0], tag) }
          : {
            ...base,
            name: string(fields[0], tag),
            typeAnnotation: fields[1],
            optional: boolean(fields[2], tag),
          };
      case 3:
        return { ...base, name: string(fields[0], tag) };
      case 4:
        return decodeLiteral(base, string(fields[0], tag), integer(fields[1], tag));
      case 5:
        return { ...base, expression: fields[0] };
      case 6:
        return { ...base, body: array(fields[0], tag) };
      case 7:
      case 8:
      case 30:
      case 71:
        return base;
      case 9:
        return { ...base, object: fields[0], body: fields[1] };
      case 10:
        return { ...base, argument: fields[0] };
      case 11:
        return { ...base, label: fields[0], body: fields[1] };
      case 12:
      case 13:
        return { ...base, label: fields[0] };
      case 14:
        return { ...base, test: fields[0], consequent: fields[1], alternate: fields[2] };
      case 15:
        return { ...base, discriminant: fields[0], cases: array(fields[1], tag) };
      case 16:
        return { ...base, test: fields[0], consequent: array(fields[1], tag) };
      case 17:
        return { ...base, argument: fields[0] };
      case 18:
        return { ...base, block: fields[0], handler: fields[1], finalizer: fields[2] };
      case 19:
        return { ...base, param: fields[0], body: fields[1] };
      case 20:
        return { ...base, test: fields[0], body: fields[1] };
      case 21:
        return { ...base, body: fields[0], test: fields[1] };
      case 22:
        return { ...base, init: fields[0], test: fields[1], update: fields[2], body: fields[3] };
      case 23:
        return { ...base, left: fields[0], right: fields[1], body: fields[2] };
      case 24:
        return {
          ...base,
          left: fields[0],
          right: fields[1],
          body: fields[2],
          await: boolean(fields[3], tag),
        };
      case 25:
      case 26:
        return {
          ...base,
          id: fields[0],
          params: array(fields[1], tag),
          body: fields[2],
          generator: boolean(fields[3], tag),
          async: boolean(fields[4], tag),
        };
      case 27:
        return {
          ...base,
          id: null,
          params: array(fields[0], tag),
          body: fields[1],
          generator: false,
          async: boolean(fields[2], tag),
          expression: boolean(fields[3], tag),
        };
      case 28:
        return {
          ...base,
          declarations: array(fields[0], tag),
          kind: enumValue(VARIABLE_KINDS, fields[1], tag),
        };
      case 29:
        return { ...base, id: fields[0], init: fields[1] };
      case 31:
        return { ...base, elements: array(fields[0], tag) };
      case 32:
        return { ...base, properties: array(fields[0], tag) };
      case 33:
        return {
          ...base,
          key: fields[0],
          value: fields[1],
          kind: enumValue(PROPERTY_KINDS, fields[2], tag),
          method: boolean(fields[3], tag),
          shorthand: boolean(fields[4], tag),
          computed: boolean(fields[5], tag),
        };
      case 34:
        return { ...base, expressions: array(fields[0], tag) };
      case 35:
        return {
          ...base,
          operator: enumValue(UNARY_OPERATORS, fields[0], tag),
          prefix: boolean(fields[1], tag),
          argument: fields[2],
        };
      case 36:
        return {
          ...base,
          operator: enumValue(UPDATE_OPERATORS, fields[0], tag),
          prefix: boolean(fields[1], tag),
          argument: fields[2],
        };
      case 37:
      case 38:
        return {
          ...base,
          operator: enumValue(BINARY_OPERATORS, fields[0], tag),
          left: fields[1],
          right: fields[2],
        };
      case 39:
        return {
          ...base,
          operator: enumValue(ASSIGNMENT_OPERATORS, fields[0], tag),
          left: fields[1],
          right: fields[2],
        };
      case 40:
        return { ...base, left: fields[0], right: fields[1] };
      case 41:
        return { ...base, test: fields[0], consequent: fields[1], alternate: fields[2] };
      case 42:
        return { ...base, callee: fields[0], arguments: array(fields[1], tag) };
      case 43:
        return {
          ...base,
          callee: fields[0],
          arguments: array(fields[1], tag),
          optional: boolean(fields[2], tag),
        };
      case 44:
        return {
          ...base,
          object: fields[0],
          property: fields[1],
          computed: boolean(fields[2], tag),
          optional: boolean(fields[3], tag),
        };
      case 45:
        return { ...base, expression: fields[0] };
      case 46:
        return { ...base, argument: fields[0], delegate: boolean(fields[1], tag) };
      case 47:
        return { ...base, argument: fields[0] };
      case 48:
        return { ...base, quasis: array(fields[0], tag), expressions: array(fields[1], tag) };
      case 49: {
        const raw = templateElementRaw(string(fields[0], tag));
        return {
          ...base,
          value: { raw, cooked: decodeQuotedString(`\"${raw}\"`) },
          tail: boolean(fields[1], tag),
        };
      }
      case 50:
        return { ...base, tag: fields[0], quasi: fields[1] };
      case 51:
      case 52:
        return { ...base, argument: fields[0] };
      case 53:
        return { ...base, elements: patternItems(fields[0], "elements", tag) };
      case 54:
        return { ...base, properties: patternItems(fields[0], "properties", tag) };
      case 55:
        return { ...base, meta: fields[0], property: fields[1] };
      case 56:
        return { ...base, source: fields[0], options: fields[1] };
      case 57:
      case 58:
        return { ...base, id: fields[0], superClass: fields[1], body: fields[2] };
      case 59:
        return { ...base, body: array(fields[0], tag) };
      case 60:
        return {
          ...base,
          key: fields[0],
          value: fields[1],
          kind: enumValue(METHOD_KINDS, fields[2], tag),
          computed: boolean(fields[3], tag),
          static: boolean(fields[4], tag),
        };
      case 61:
        return {
          ...base,
          key: fields[0],
          value: fields[1],
          computed: boolean(fields[2], tag),
          static: boolean(fields[3], tag),
          typeAnnotation: fields[4],
        };
      case 62: {
        const block = fields[0];
        if (block?.type !== "BlockStatement") {
          throw new Error("JetSyntax StaticBlock expected a BlockStatement field");
        }
        return { ...base, body: block.body };
      }
      case 63:
        return {
          ...base,
          specifiers: array(fields[0], tag),
          source: fields[1],
          attributes: array(fields[2], tag),
          importKind: enumValue(IMPORT_EXPORT_KINDS, fields[3], tag),
        };
      case 64:
        return {
          ...base,
          imported: fields[0],
          local: fields[1],
          importKind: enumValue(IMPORT_EXPORT_KINDS, fields[2], tag),
        };
      case 65:
      case 66:
        return { ...base, local: fields[0] };
      case 67:
        return {
          ...base,
          declaration: fields[0],
          specifiers: array(fields[1], tag),
          source: fields[2],
          attributes: array(fields[3], tag),
          exportKind: enumValue(IMPORT_EXPORT_KINDS, fields[4], tag),
        };
      case 68:
        return { ...base, declaration: fields[0] };
      case 69:
        return {
          ...base,
          source: fields[0],
          exported: fields[1],
          attributes: array(fields[2], tag),
          exportKind: enumValue(IMPORT_EXPORT_KINDS, fields[3], tag),
        };
      case 70:
        return { ...base, local: fields[0], exported: fields[1] };
      case 72:
        return { ...base, expression: fields[0] };
      case 256:
        return { ...base, name: string(fields[0], tag) };
      case 259:
        return {
          ...base,
          openingElement: fields[0],
          closingElement: fields[1],
          children: array(fields[2], tag),
        };
      case 261:
        return {
          ...base,
          name: fields[0],
          attributes: array(fields[1], tag),
          selfClosing: boolean(fields[2], tag),
        };
      case 262:
        return { ...base, name: fields[0] };
      case 265:
        return { ...base, name: fields[0], value: fields[1] };
      case 266:
        return { ...base, argument: fields[0] };
      case 267:
        return { ...base, expression: fields[0] };
      case 268:
        return base;
      case 269: {
        const raw = string(fields[0], tag);
        return { ...base, value: raw, raw };
      }
      case 512:
        return { ...base, typeAnnotation: fields[0] };
      case 513:
        return { ...base, typeName: fields[0], typeParameters: fields[1] };
      case 514:
        return { ...base, left: fields[0], right: fields[1] };
      default:
        throw new Error(`missing ESTree decoder for JetSyntax node tag ${tag}`);
    }
  }

  return readValue(header.root);
}

function validateHeader(tape) {
  if (tape.length < HEADER_WORDS) throw new Error("truncated JetSyntax tape header");
  if (tape[0] !== MAGIC) throw new Error("invalid JetSyntax tape magic");
  if (tape[1] !== FORMAT_VERSION) {
    throw new Error(`unsupported JetSyntax tape version ${tape[1]}`);
  }
  if (tape[2] !== HEADER_WORDS) throw new Error("invalid JetSyntax tape header size");
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
  return { recordEnd, poolBytes, root: tape[HEADER_ROOT] };
}

function indexRecords(tape, recordEnd) {
  const starts = new Set();
  let offset = HEADER_WORDS;
  let last = 0;
  while (offset < recordEnd) {
    starts.add(offset);
    last = offset;
    const record = tape[offset];
    const kind = (record & KIND_MASK) >>> KIND_SHIFT;
    let size;
    if (kind === KIND_NODE) {
      requireWords(offset, 5, recordEnd);
      size = 5 + tape[offset + 4];
      if (tape[offset + 1] !== size) throw new Error(`invalid JetSyntax node length at word ${offset}`);
    } else if (kind === KIND_LIST) {
      requireWords(offset, 3, recordEnd);
      size = 3 + tape[offset + 2];
      if (record !== KIND_LIST << KIND_SHIFT || tape[offset + 1] !== size) {
        throw new Error(`invalid JetSyntax list record at word ${offset}`);
      }
    } else {
      size = scalarSize(kind, record, offset);
    }
    requireWords(offset, size, recordEnd);
    offset += size;
  }
  if (offset !== recordEnd) throw new Error("JetSyntax record section ends inside a record");
  starts.last = last;
  return starts;
}

function scalarSize(kind, record, offset) {
  switch (kind) {
    case KIND_NULL:
      if (record !== (KIND_NULL << KIND_SHIFT) >>> 0) break;
      return 1;
    case KIND_BOOL:
      if ((record & ~1) !== KIND_BOOL << KIND_SHIFT) break;
      return 1;
    case KIND_INLINE_U32:
      return 1;
    case KIND_U32:
      if (record !== (KIND_U32 << KIND_SHIFT) >>> 0) break;
      return 2;
    case KIND_F64:
      if (record !== (KIND_F64 << KIND_SHIFT) >>> 0) break;
      return 3;
    case KIND_SOURCE_SLICE:
      if (record !== (KIND_SOURCE_SLICE << KIND_SHIFT) >>> 0) break;
      return 3;
    case KIND_POOL_STRING:
      if (record !== (KIND_POOL_STRING << KIND_SHIFT) >>> 0) break;
      return 3;
  }
  throw new Error(`unknown or malformed JetSyntax value at word ${offset}`);
}

function requireWords(offset, size, recordEnd) {
  if (!Number.isSafeInteger(size) || size <= 0 || offset + size > recordEnd) {
    throw new Error(`truncated JetSyntax record at word ${offset}`);
  }
}

function makeSourceOffsets(source, byteLength) {
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

function decodeLiteral(base, raw, kind) {
  switch (kind) {
    case 0:
      return { ...base, value: Number(raw.replaceAll("_", "")), raw };
    case 1:
      return { ...base, value: decodeQuotedString(raw), raw };
    case 2:
      return { ...base, value: raw === "true", raw };
    case 3:
      return { ...base, value: null, raw };
    case 4: {
      const bigint = raw.replaceAll("_", "").replace(/n$/u, "");
      return { ...base, value: BigInt(bigint), bigint, raw };
    }
    case 5:
      return { ...base, value: decodeQuotedString(`\"${raw.slice(1, -1)}\"`), raw };
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
      return { ...base, value, regex: { pattern, flags }, raw };
    }
    default:
      throw new Error(`unsupported JetSyntax literal kind ${kind}`);
  }
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
