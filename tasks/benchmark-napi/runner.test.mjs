import assert from "node:assert/strict";
import test from "node:test";

import { measureInterleaved } from "./runner.mjs";

test("measurement rotates parser order while retaining per-parser samples", () => {
  const order = [];
  let clock = 0;
  const results = measureInterleaved(
    [
      {
        name: "first",
        run: () => {
          order.push("first");
          clock += 1;
        },
      },
      {
        name: "second",
        run: () => {
          order.push("second");
          clock += 2;
        },
      },
    ],
    { warmups: 1, samples: 3, now: () => clock },
  );

  assert.deepEqual(order, ["first", "second", "first", "second", "second", "first", "first", "second"]);
  assert.deepEqual(results.get("first"), { medianMs: 1, minimumMs: 1, p99Ms: 1 });
  assert.deepEqual(results.get("second"), { medianMs: 2, minimumMs: 2, p99Ms: 2 });
});

test("measurement uses the default performance clock", () => {
  const results = measureInterleaved([{ name: "parser", run() {} }], { warmups: 0, samples: 1 });

  assert.ok(Number.isFinite(results.get("parser").medianMs));
});

test("measurement rejects an empty parser set", () => {
  assert.throws(
    () => measureInterleaved([], { warmups: 1, samples: 1 }),
    /at least one parser is required/,
  );
});
