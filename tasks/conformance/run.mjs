import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { basename, dirname, extname, join, resolve } from "node:path";

import { parse } from "jetsyntax";

const suites = [
  { path: "js/pass", expectation: "snapshot", lang: "js", semanticErrors: true },
  { path: "js/fail", expectation: "fail", lang: "js", semanticErrors: true },
  { path: "js/semantic", expectation: "fail", lang: "js", semanticErrors: true },
  { path: "jsx/pass", expectation: "snapshot", lang: "jsx", semanticErrors: true },
  { path: "jsx/fail", expectation: "fail", lang: "jsx", semanticErrors: true },
  { path: "jsx/semantic", expectation: "fail", lang: "jsx", semanticErrors: true },
  // TypeScript's parser-pass corpus intentionally contains programs that fail later compiler
  // checks. Keep grammar conformance separate from the explicit `ts/semantic` early-error suite.
  { path: "ts/pass", expectation: "snapshot", lang: "ts", semanticErrors: false },
  { path: "ts/semantic", expectation: "fail", lang: "ts", semanticErrors: true },
];

const arguments_ = parseArguments(process.argv.slice(2));
const suiteRoot = resolve(arguments_.suite ?? process.env.JETSYNTAX_SUITE ?? "vendor/parser-test-suite");
const shardIndex = Number(process.env.SHARD_INDEX ?? 0);
const shardTotal = Number(process.env.SHARD_TOTAL ?? 1);
const failures = [];
let discovered = 0;
let executed = 0;
let passed = 0;
let snapshotsExpected = 0;
let snapshotsCompared = 0;
let snapshotsMissing = 0;

if (
  !Number.isInteger(shardIndex) || !Number.isInteger(shardTotal) || shardTotal < 1 || shardIndex < 0
  || shardIndex >= shardTotal
) {
  throw new Error(`invalid shard ${shardIndex}/${shardTotal}`);
}

for (const suite of suites) {
  const root = join(suiteRoot, suite.path);
  const entries = await readdir(root, { recursive: true, withFileTypes: true });
  const files = entries
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .filter((file) => isSourceFile(file))
    .sort();

  discovered += files.length;
  for (const file of files) {
    const relativePath = file.slice(suiteRoot.length + 1);
    if (hash(relativePath) % shardTotal !== shardIndex) continue;
    if (arguments_.filter && !relativePath.includes(arguments_.filter)) continue;
    if (arguments_.limit !== undefined && executed >= arguments_.limit) break;

    executed++;
    const source = await readSource(file);
    const options = {
      lang: languageFor(file, suite.lang),
      sourceType: file.includes(".module.") ? "module" : "script",
      preserveParens: true,
      semanticErrors: suite.semanticErrors,
    };
    const result = parse(source, options);

    try {
      if (suite.expectation === "fail") {
        if (result.diagnostics.length === 0) throw new Error("expected a diagnostic");
      } else {
        if (result.diagnostics.length > 0) throw new Error(result.diagnostics[0]);
        snapshotsExpected++;
        const snapshotPath = join(dirname(file), "snapshots", `${baseName(file)}.snapshot.json`);
        try {
          const snapshot = JSON.parse(await readFile(snapshotPath, "utf8"), reviveSnapshotValue);
          assertAstMatches(result.program, snapshot.program);
          snapshotsCompared++;
        } catch (error) {
          if (error?.code !== "ENOENT") throw error;
          snapshotsMissing++;
        }
      }
      passed++;
    } catch (error) {
      failures.push({ file: relativePath, reason: error instanceof Error ? error.message : String(error) });
      if (failures.length <= 20) console.error(`FAIL ${relativePath}: ${failures.at(-1).reason}`);
    }
  }
}

const report = {
  suiteCommit: process.env.SUITE_COMMIT ?? "local",
  shard: { index: shardIndex, total: shardTotal },
  discovered,
  executed,
  passed,
  failed: failures.length,
  skipped: 0,
  snapshots: {
    expected: snapshotsExpected,
    compared: snapshotsCompared,
    missing: snapshotsMissing,
  },
  failures,
};
const reportPath = resolve(process.env.CONFORMANCE_REPORT ?? `reports/conformance-${shardIndex}.json`);
await mkdir(dirname(reportPath), { recursive: true });
await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ ...report, failures: failures.slice(0, 20) }, null, 2));

if (executed === 0) throw new Error("conformance shard executed no tests");
if (failures.length > 0) process.exitCode = 1;

function parseArguments(values) {
  const output = {};
  for (let index = 0; index < values.length; index++) {
    const value = values[index];
    if (value === "--") continue;
    if (value === "--suite") output.suite = values[++index];
    else if (value === "--filter") output.filter = values[++index];
    else if (value === "--limit") output.limit = Number(values[++index]);
    else throw new Error(`unknown argument: ${value}`);
  }
  return output;
}

function isSourceFile(file) {
  if (file.includes("/snapshots/") || file.endsWith(".snapshot.json")) return false;
  return [".js", ".jsx", ".ts", ".tsx"].includes(extname(file));
}

async function readSource(file) {
  const bytes = await readFile(file);
  if (bytes[0] === 0xFF && bytes[1] === 0xFE) {
    return new TextDecoder("utf-16le").decode(bytes.subarray(2));
  }
  if (bytes[0] === 0xFE && bytes[1] === 0xFF) {
    return new TextDecoder("utf-16be").decode(bytes.subarray(2));
  }
  return bytes.toString("utf8");
}

function languageFor(file, fallback) {
  if (file.endsWith(".d.ts") || file.endsWith(".d.mts") || file.endsWith(".d.cts")) return "dts";
  if (file.endsWith(".tsx")) return "tsx";
  return fallback;
}

function baseName(file) {
  return basename(file).split(".", 1)[0];
}

function hash(value) {
  let output = 0x81_1C_9D_C5;
  for (let index = 0; index < value.length; index++) {
    output ^= value.charCodeAt(index);
    output = Math.imul(output, 0x01_00_01_93);
  }
  return output >>> 0;
}

function reviveSnapshotValue(_key, value) {
  if (typeof value !== "string") return value;
  if (value.startsWith("(BigInt) ")) return BigInt(value.slice(9).replace(/n$/, "").replaceAll("_", ""));
  if (value.startsWith("(Number) ")) return Number(value.slice(9));
  if (!value.startsWith("(RegExp) ")) return value;

  const match = /^\/(.*)\/([dgimsuvy]*)$/.exec(value.slice(9));
  if (!match) return null;
  try {
    return new RegExp(match[1], match[2]);
  } catch {
    return null;
  }
}

function assertAstMatches(actual, expected, path = "program") {
  if (Object.is(actual, expected)) return;
  if (actual instanceof RegExp || expected instanceof RegExp) {
    if (
      actual instanceof RegExp && expected instanceof RegExp && actual.source === expected.source
      && actual.flags === expected.flags
    ) {
      return;
    }
    throw new Error(`${path}: regular expressions differ`);
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length) {
      throw new Error(`${path}: expected array length ${expected.length}, received ${actual?.length}`);
    }
    for (let index = 0; index < expected.length; index++) {
      assertAstMatches(actual[index], expected[index], `${path}[${index}]`);
    }
    return;
  }
  if (expected === null || typeof expected !== "object") {
    throw new Error(`${path}: expected ${show(expected)}, received ${show(actual)}`);
  }
  if (actual === null || typeof actual !== "object") {
    throw new Error(`${path}: expected object, received ${show(actual)}`);
  }

  const skipRaw = expected.type === "Literal"
    && (typeof expected.value === "string" || typeof expected.value === "bigint" || "regex" in expected);
  for (const key of Object.keys(expected)) {
    if (key === "start" || key === "end" || key === "comments" || (skipRaw && key === "raw")) continue;
    if (!(key in actual)) throw new Error(`${path}: missing reference field ${key}`);
    assertAstMatches(actual[key], expected[key], `${path}.${key}`);
  }
}

function show(value) {
  if (typeof value === "bigint") return `${value}n`;
  if (Number.isNaN(value)) return "NaN";
  return JSON.stringify(value);
}
