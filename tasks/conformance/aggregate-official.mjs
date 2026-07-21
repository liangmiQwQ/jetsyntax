import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

const root = resolve(process.argv[2] ?? "reports");
const outputPath = resolve(process.env.CONFORMANCE_SUMMARY ?? join(root, "official-summary.json"));
const files = (await readdir(root, { recursive: true, withFileTypes: true }))
  .filter((entry) => entry.isFile() && /^official-(?:babel|test262|typescript)-\d+\.json$/.test(entry.name))
  .map((entry) => join(entry.parentPath, entry.name));
if (files.length === 0) throw new Error(`no official conformance reports under ${root}`);

const reports = await Promise.all(files.map(async (file) => JSON.parse(await readFile(file, "utf8"))));
const refs = JSON.parse(await readFile(new URL("./official-refs.json", import.meta.url), "utf8"));
const suiteNames = Object.keys(refs).filter((key) => key !== "schemaVersion");
const groups = new Map();
for (const report of reports) {
  const suiteReports = groups.get(report.suite) ?? [];
  suiteReports.push(report);
  groups.set(report.suite, suiteReports);
}
const suites = {};
let invalid = false;

for (const suite of suiteNames) {
  const suiteReports = groups.get(suite);
  if (!suiteReports) throw new Error(`no ${suite} official conformance reports under ${root}`);
  suiteReports.sort((left, right) => left.shard.index - right.shard.index);
  const shardTotal = suiteReports[0].shard.total;
  const expectedIndexes = Array.from({ length: shardTotal }, (_, index) => index);
  const actualIndexes = suiteReports.map((report) => report.shard.index);
  if (JSON.stringify(actualIndexes) !== JSON.stringify(expectedIndexes)) {
    throw new Error(`${suite} reports cover shards ${actualIndexes.join(",")}, expected ${expectedIndexes.join(",")}`);
  }

  const first = suiteReports[0];
  const definition = refs[suite];
  for (const report of suiteReports) {
    if (
      report.schemaVersion !== 1 || report.repository !== definition.repository || report.ref !== definition.ref
      || report.shard.total !== shardTotal || report.discovered !== first.discovered
    ) {
      throw new Error(`${suite} shard metadata disagrees`);
    }
    assertInventory(report.inventory, definition.inventory, suite);
    if (report.skipped !== 0) throw new Error(`${suite} shard ${report.shard.index} skipped tests`);
  }
  if (first.discovered !== definition.inventory.executions) {
    throw new Error(`${suite} discovered ${first.discovered}/${definition.inventory.executions} pinned cases`);
  }

  const totals = sumFields(suiteReports, ["executed", "passed", "failed", "unsupported", "skipped"]);
  if (totals.executed !== first.discovered) {
    throw new Error(`${suite} executed ${totals.executed}/${first.discovered} discovered cases`);
  }
  if (totals.executed !== totals.passed + totals.failed + totals.unsupported) {
    throw new Error(`${suite} result accounting does not add up`);
  }
  if (totals.failed > 0 || totals.unsupported > 0 || totals.skipped > 0) invalid = true;
  suites[suite] = {
    repository: first.repository,
    ref: first.ref,
    inventory: first.inventory,
    shards: shardTotal,
    ...totals,
  };
}

const summary = { schemaVersion: 1, suites };
await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(summary, null, 2)}\n`);
console.log(JSON.stringify(summary, null, 2));
if (invalid) process.exitCode = 1;

function sumFields(reports, fields) {
  return Object.fromEntries(fields.map((field) => [field, reports.reduce((sum, report) => sum + report[field], 0)]));
}

function assertInventory(actual, expected, suite) {
  for (const [key, value] of Object.entries(expected)) {
    if (actual?.[key] !== value) {
      throw new Error(`${suite} inventory changed: ${key} is ${actual?.[key]}, expected ${value}`);
    }
  }
}
