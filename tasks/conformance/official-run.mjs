import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

import { parse } from "jetsyntax";

import { loadBabel } from "./official/babel.mjs";
import { hash } from "./official/shared.mjs";
import { loadTest262 } from "./official/test262.mjs";
import { loadTypeScript } from "./official/typescript.mjs";

const loaders = { babel: loadBabel, test262: loadTest262, typescript: loadTypeScript };
const arguments_ = parseArguments(process.argv.slice(2));
const refs = JSON.parse(await readFile(new URL("./official-refs.json", import.meta.url), "utf8"));
const definition = refs[arguments_.suite];
if (!definition) throw new Error(`unknown official suite: ${arguments_.suite}`);
if (arguments_.ref && arguments_.ref !== definition.ref) {
  throw new Error(`checked out ${arguments_.ref}, expected ${definition.ref} for ${arguments_.suite}`);
}

const shardIndex = Number(process.env.SHARD_INDEX ?? 0);
const shardTotal = Number(process.env.SHARD_TOTAL ?? 1);
if (
  !Number.isInteger(shardIndex) || !Number.isInteger(shardTotal) || shardTotal < 1 || shardIndex < 0
  || shardIndex >= shardTotal
) {
  throw new Error(`invalid shard ${shardIndex}/${shardTotal}`);
}

const loaded = await loaders[arguments_.suite](resolve(arguments_.root));
assertInventory(loaded.inventory, definition.inventory, arguments_.suite);

const failures = [];
const unsupportedReasons = {};
let executed = 0;
let passed = 0;
let failed = 0;
let unsupported = 0;

for (const testCase of loaded.cases) {
  const shardKey = testCase.project ?? testCase.id;
  if (hash(shardKey) % shardTotal !== shardIndex) continue;
  executed++;

  let result;
  let parseError;
  try {
    result = parse(testCase.source, testCase.options);
  } catch (error) {
    parseError = error instanceof Error ? error.message : String(error);
  }

  if (testCase.unsupportedReasons?.length > 0) {
    unsupported++;
    for (const reason of testCase.unsupportedReasons) {
      unsupportedReasons[reason] = (unsupportedReasons[reason] ?? 0) + 1;
    }
    recordFailure(failures, testCase, `unsupported extension: ${testCase.unsupportedReasons.join(", ")}`);
    continue;
  }

  try {
    if (parseError) throw new Error(parseError);
    assertResult(result, testCase.expectation);
    passed++;
  } catch (error) {
    failed++;
    recordFailure(failures, testCase, error instanceof Error ? error.message : String(error));
  }
}

const report = {
  schemaVersion: 1,
  suite: arguments_.suite,
  repository: definition.repository,
  ref: definition.ref,
  shard: { index: shardIndex, total: shardTotal },
  inventory: loaded.inventory,
  extensions: loaded.extensions ?? {},
  discovered: loaded.cases.length,
  executed,
  passed,
  failed,
  unsupported,
  unsupportedReasons,
  skipped: 0,
  failures,
};
const reportPath = resolve(
  process.env.CONFORMANCE_REPORT ?? `reports/official-${arguments_.suite}-${shardIndex}.json`,
);
await mkdir(dirname(reportPath), { recursive: true });
await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ ...report, failures: failures.slice(0, 20) }, null, 2));

if (executed === 0) throw new Error("official conformance shard executed no tests");
if (failed > 0 || unsupported > 0) process.exitCode = 1;

function parseArguments(values) {
  const output = {};
  for (let index = 0; index < values.length; index++) {
    const value = values[index];
    if (value === "--") continue;
    if (value === "--suite") output.suite = values[++index];
    else if (value === "--root") output.root = values[++index];
    else if (value === "--ref") output.ref = values[++index];
    else throw new Error(`unknown argument: ${value}`);
  }
  if (!output.suite || !output.root) throw new Error("--suite and --root are required");
  return output;
}

function assertInventory(actual, expected, suite) {
  for (const [key, value] of Object.entries(expected)) {
    if (actual[key] !== value) {
      throw new Error(`${suite} inventory changed: ${key} is ${actual[key]}, expected ${value}`);
    }
  }
}

function assertResult(result, expectation) {
  if (!result || typeof result !== "object") throw new Error("parser returned no result");
  if (result.panicked) throw new Error("parser panicked");
  const diagnostics = result.diagnostics ?? [];
  if (expectation === "clean") {
    if (diagnostics.length > 0) throw new Error(diagnostics[0]);
  } else if (diagnostics.length === 0) {
    throw new Error(`expected ${expectation} diagnostic`);
  }
  if (expectation !== "fatal") {
    if (result.program?.type !== "Program" || !Array.isArray(result.program.body)) {
      throw new Error("parser returned an invalid ESTree Program");
    }
  }
}

function recordFailure(failures, testCase, reason) {
  failures.push({ id: testCase.id, path: testCase.path, reason });
  if (failures.length <= 20) console.error(`FAIL ${testCase.id}: ${reason}`);
}
