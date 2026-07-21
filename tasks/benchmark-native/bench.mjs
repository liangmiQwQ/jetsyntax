import { spawnSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import { prepareFixtures } from "../benchmark-napi/prepare.mjs";
import { prepareYuku } from "./prepare-yuku.mjs";

const root = resolve(import.meta.dirname, "../..");
const warmups = positiveInteger("BENCH_WARMUPS", 50);
const samples = positiveInteger("BENCH_SAMPLES", 300);

// 1. Build both language runners against the exact local source revisions.
const fixtures = await prepareFixtures();
const paths = Object.values(fixtures).map((fixture) => fixture.path);
const yuku = await prepareYuku();
const rustFlags = [process.env.RUSTFLAGS, "-C target-cpu=native"].filter(Boolean).join(" ");
run("cargo", ["build", "--quiet", "--release", "-p", "jetsyntax_benchmark_native"], {
  cwd: root,
  env: { ...process.env, RUSTFLAGS: rustFlags },
});

// 2. Run each process once so process startup is excluded from every measured sample.
const rust = runJson(resolve(root, "target/release/jetsyntax_benchmark_native"), paths, { cwd: root });
const yukuResult = runJson(yuku.binary, [String(warmups), String(samples), ...paths], { cwd: root });
if (rust.methodology.warmups !== warmups || rust.methodology.samples !== samples) {
  throw new Error("Rust benchmark methodology did not match the requested sample counts");
}
if (yukuResult.methodology.warmups !== warmups || yukuResult.methodology.samples !== samples) {
  throw new Error("Yuku benchmark methodology did not match the requested sample counts");
}

// 3. Persist exact inputs, revisions, raw statistics, and the speed gate in one report.
const results = [...rust.results, ...yukuResult.results];
const comparisons = Object.values(fixtures).map((fixture) => {
  const jetSyntax = findResult(results, fixture.path, "JetSyntax");
  const currentYuku = findResult(results, fixture.path, "Yuku");
  const fasterPercent = (currentYuku.medianMs / jetSyntax.medianMs - 1) * 100;
  return {
    fixture: fixtureName(fixture.path),
    jetSyntaxMedianMs: jetSyntax.medianMs,
    yukuMedianMs: currentYuku.medianMs,
    fasterPercent,
    passesTenPercentGate: fasterPercent >= 10,
  };
});
const report = {
  generatedAt: new Date().toISOString(),
  platform: `${process.platform}-${process.arch}`,
  methodology: {
    warmups,
    samples,
    statistic: "median wall-clock time",
    speedup: "(Yuku median / JetSyntax median - 1) * 100",
    materialization: "parse plus access to the returned syntax-tree storage length",
    allocation: "parser-owned output allocation is included; output destruction is excluded",
  },
  buildFlags: {
    Rust: `cargo build --release; RUSTFLAGS=${rustFlags}`,
    Yuku: yuku.flags,
  },
  revisions: { JetSyntax: git(root), Yuku: yuku.commit, Zig: yuku.zigVersion },
  fixtures: Object.fromEntries(
    Object.entries(fixtures).map(([name, fixture]) => [
      name,
      { source: fixture.url, sha256: fixture.sha256, bytes: fixture.bytes },
    ]),
  ),
  results,
  comparisons,
};
await mkdir(resolve(root, "reports"), { recursive: true });
await writeFile(
  resolve(root, "reports/benchmark-native.json"),
  `${JSON.stringify(report, null, 2)}\n`,
);

for (const comparison of comparisons) {
  console.log(
    `${comparison.fixture}: JetSyntax ${comparison.jetSyntaxMedianMs.toFixed(3)} ms, Yuku ${
      comparison.yukuMedianMs.toFixed(3)
    } ms (${comparison.fasterPercent.toFixed(1)}% faster)`,
  );
}
if (process.env.BENCH_ENFORCE === "1" && comparisons.some((comparison) => !comparison.passesTenPercentGate)) {
  throw new Error("JetSyntax did not clear the 10% native Yuku speed gate on every fixture");
}

function runJson(command, arguments_, options) {
  return JSON.parse(run(command, arguments_, options));
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    encoding: "utf8",
    env: { ...process.env, BENCH_WARMUPS: String(warmups), BENCH_SAMPLES: String(samples) },
    maxBuffer: 20 * 1024 * 1024,
    stdio: "pipe",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(
      `${command} failed (${result.status}):\n${result.error?.message || result.stderr || result.stdout}`,
    );
  }
  if (result.stderr) process.stderr.write(result.stderr);
  return result.stdout;
}

function findResult(results, path, parser) {
  const fixture = fixtureName(path);
  const result = results.find((entry) => entry.fixture === fixture && entry.parser === parser);
  if (!result) throw new Error(`missing ${parser} result for ${fixture}`);
  return result;
}

function fixtureName(path) {
  return path.split("/").at(-1);
}

function positiveInteger(name, fallback) {
  const value = Number.parseInt(process.env[name] ?? String(fallback), 10);
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
  return value;
}

function git(directory) {
  try {
    return run("git", ["-C", directory, "rev-parse", "--verify", "HEAD"]).trim();
  } catch {
    return "uncommitted";
  }
}
