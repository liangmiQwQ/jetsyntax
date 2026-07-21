import { spawnSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

import swc from "@swc/core";
import { parse as parseJetSyntax } from "jetsyntax";
import { parseSync as parseOxc } from "oxc-parser";
import { parse as parseYuku } from "yuku-parser";

import { compareWithYuku, positiveInteger } from "./metrics.mjs";
import { prepareFixtures } from "./prepare.mjs";
import { measureInterleaved } from "./runner.mjs";

const require = createRequire(import.meta.url);
const root = resolve(import.meta.dirname, "../..");
const warmups = positiveInteger(process.env.BENCH_WARMUPS ?? "50", "BENCH_WARMUPS");
const samples = positiveInteger(process.env.BENCH_SAMPLES ?? "300", "BENCH_SAMPLES");
const thresholdPercent = 10;
const parsers = parserMetadata();
const prepared = await prepareFixtures();
const fixtures = [
  {
    name: "typescript-5.1.6",
    filename: "typescript.js",
    lang: "js",
    source: prepared.typescript.source,
  },
  {
    name: "checker-c9e7428",
    filename: "checker.ts",
    lang: "ts",
    source: prepared.checker.source,
  },
  {
    name: "react-17.0.2",
    filename: "react.development.js",
    lang: "js",
    source: prepared.react.source,
  },
];

const results = [];
for (const fixture of fixtures) {
  const parsers = parserCases(fixture);
  for (const parser of parsers) validate(parser, fixture);

  globalThis.gc?.();
  const measurements = measureInterleaved(
    parsers.map((parser) => ({ name: parser.name, run: () => parser.parse(fixture.source) })),
    { initialOffset: fixtures.indexOf(fixture), samples, warmups },
  );
  for (const parser of parsers) {
    const stats = measurements.get(parser.name);
    const result = {
      fixture: fixture.name,
      bytes: Buffer.byteLength(fixture.source),
      parser: parser.name,
      ...stats,
      megabytesPerSecond: (Buffer.byteLength(fixture.source) / 1_000_000) / (stats.medianMs / 1_000),
    };
    results.push(result);
    console.log(`${fixture.name.padEnd(20)} ${parser.name.padEnd(10)} ${stats.medianMs.toFixed(3)} ms`);
  }
}

const comparisons = compareWithYuku(results, thresholdPercent);
const gatePassed = comparisons.every((comparison) => comparison.passes);

const report = {
  generatedAt: new Date().toISOString(),
  runtime: process.version,
  platform: `${process.platform}-${process.arch}`,
  methodology: {
    warmups,
    samples,
    statistic: "median wall-clock latency",
    speedup: "(Yuku median / JetSyntax median - 1) * 100",
    materialization: "synchronous parse plus access to the returned Program body",
  },
  parsers,
  fixtures: Object.fromEntries(
    Object.entries(prepared).map(([name, fixture]) => [
      name,
      { source: fixture.url, sha256: fixture.sha256, bytes: fixture.bytes },
    ]),
  ),
  results,
  comparisons,
  gate: { thresholdPercent, enforced: process.env.BENCH_ENFORCE === "1", passed: gatePassed },
};
const reportDirectory = resolve(root, "reports");
await mkdir(reportDirectory, { recursive: true });
await writeFile(join(reportDirectory, "benchmark-napi.json"), `${JSON.stringify(report, null, 2)}\n`);
for (const comparison of comparisons) {
  console.log(
    `${comparison.fixture.padEnd(20)} JetSyntax vs Yuku ${comparison.fasterPercent.toFixed(1)}% ${
      comparison.passes ? "PASS" : "FAIL"
    }`,
  );
}
if (process.env.BENCH_ENFORCE === "1" && !gatePassed) {
  throw new Error("JetSyntax did not clear the 10% NAPI Yuku speed gate on every fixture");
}

function parserCases(fixture) {
  const isTypeScript = fixture.lang === "ts";
  return [
    {
      name: "JetSyntax",
      parse: (source) => parseJetSyntax(source, { lang: fixture.lang }).program.body.length,
      validate: (source) => {
        const result = parseJetSyntax(source, { lang: fixture.lang });
        if (result.diagnostics.length !== 0) {
          throw new Error(`JetSyntax emitted ${result.diagnostics.length} diagnostics for ${fixture.name}`);
        }
        return result.program.body.length;
      },
    },
    {
      name: "Yuku",
      parse: (source) => parseYuku(source, isTypeScript ? { lang: "ts" } : undefined).program.body.length,
    },
    {
      name: "OXC",
      parse: (source) => parseOxc(fixture.filename, source).program.body.length,
    },
    {
      name: "SWC",
      parse: (source) =>
        swc.parseSync(source, isTypeScript ? { syntax: "typescript" } : { syntax: "ecmascript" }).body.length,
    },
  ];
}

function validate(parser, fixture) {
  const bodyLength = (parser.validate ?? parser.parse)(fixture.source);
  if (!Number.isInteger(bodyLength) || bodyLength < 1) {
    throw new Error(`${parser.name} did not materialize ${fixture.name}`);
  }
}

function parserMetadata() {
  const pins = {
    Yuku: {
      package: "yuku-parser",
      version: "0.7.2",
      sourceTag: "v0.7.2",
      sourceRevision: "217133c5db1cb65bc8f0a44281c505cd46fa7a96",
    },
    OXC: {
      package: "oxc-parser",
      version: "0.140.0",
      sourceTag: "crates_v0.140.0",
      sourceRevision: "8e0ed2ebb96137fb1611cdbd5742d5cb46037d40",
    },
    SWC: {
      package: "@swc/core",
      version: "1.15.46",
      sourceTag: "v1.15.46",
      sourceRevision: "93831621baf6fe999ec5fcc6fc96388f072c19f1",
    },
  };
  for (const pin of Object.values(pins)) {
    const installed = require(`${pin.package}/package.json`).version;
    if (installed !== pin.version) {
      throw new Error(`expected ${pin.package}@${pin.version}, found ${installed}`);
    }
  }

  return {
    JetSyntax: {
      package: "jetsyntax",
      version: require("jetsyntax/package.json").version,
      sourceRevision: gitRevision(),
    },
    ...pins,
  };
}

function gitRevision() {
  const result = spawnSync("git", ["-C", root, "rev-parse", "--verify", "HEAD"], {
    encoding: "utf8",
  });
  return result.status === 0 ? result.stdout.trim() : "uncommitted";
}
