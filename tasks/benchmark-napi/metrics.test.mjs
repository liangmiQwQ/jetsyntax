import assert from "node:assert/strict";
import test from "node:test";

import { compareWithYuku, positiveInteger } from "./metrics.mjs";

test("comparison uses throughput-equivalent speedup and includes the ten-percent boundary", () => {
  const comparisons = compareWithYuku([
    { fixture: "exact-boundary", parser: "JetSyntax", medianMs: 100 },
    { fixture: "exact-boundary", parser: "Yuku", medianMs: 110 },
    { fixture: "below-boundary", parser: "JetSyntax", medianMs: 100 },
    { fixture: "below-boundary", parser: "Yuku", medianMs: 109 },
  ]);

  assert.equal(comparisons[0].passes, true);
  assert.ok(Math.abs(comparisons[0].fasterPercent - 10) < 1e-10);
  assert.equal(comparisons[1].passes, false);
});

test("comparison rejects missing or duplicate parser results", () => {
  assert.throws(
    () => compareWithYuku([{ fixture: "missing", parser: "JetSyntax", medianMs: 1 }]),
    /expected one Yuku result/,
  );
  assert.throws(
    () =>
      compareWithYuku([
        { fixture: "duplicate", parser: "JetSyntax", medianMs: 1 },
        { fixture: "duplicate", parser: "Yuku", medianMs: 2 },
        { fixture: "duplicate", parser: "Yuku", medianMs: 2 },
      ]),
    /found 2/,
  );
});

test("sample counts accept only canonical positive integers", () => {
  assert.equal(positiveInteger("50", "BENCH_WARMUPS"), 50);
  for (const value of ["0", "-1", "1.5", "abc", "01"]) {
    assert.throws(() => positiveInteger(value, "BENCH_SAMPLES"), /positive integer/);
  }
});
