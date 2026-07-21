import assert from "node:assert/strict";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { loadBabel } from "../official/babel.mjs";
import { loadTest262 } from "../official/test262.mjs";
import { loadTypeScript } from "../official/typescript.mjs";

test("official loaders preserve suite outcomes and explicit exclusions", async () => {
  const root = await mkdtemp(join(tmpdir(), "jetsyntax-official-loaders-"));

  const test262Root = join(root, "test262", "test", "language");
  await write(test262Root, "clean.js", `/*---\ndescription: clean\n---*/\nlet value = 1;`);
  await write(
    test262Root,
    "negative.js",
    `/*---\nflags:\n  - onlyStrict\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nlet let;`,
  );
  await write(test262Root, "module_FIXTURE.js", "export const value = 1;");
  const test262 = await loadTest262(join(root, "test262", "test"));
  assert.deepEqual(test262.inventory, {
    standaloneFiles: 2,
    fixtureFiles: 1,
    executions: 3,
    parseNegativeFiles: 1,
  });

  const babelRoot = join(root, "babel", "fixtures");
  await write(join(babelRoot, "flow"), "options.json", JSON.stringify({ plugins: ["flow"] }));
  await babelFixture(babelRoot, "flow/syntax/flow", "type Value = number;", { output: {} });
  await babelFixture(babelRoot, "core/syntax/clean", "let value = 1;", { output: {} });
  await babelFixture(babelRoot, "core/syntax/recovery", "let value = ;", { output: { errors: ["SyntaxError"] } });
  await babelFixture(babelRoot, "core/syntax/fatal", "let value = ;", { options: { throws: "Unexpected token" } });
  await babelFixture(babelRoot, "core/syntax/.disabled", "let value = 1;", { output: {} });
  await babelFixture(babelRoot, "typescript/dts/implementation", "function foo(): any {}", {
    options: { plugins: [["typescript", { dts: true }]] },
    output: { errors: ["A function implementation cannot be declared in an ambient context."] },
  });
  await babelFixture(babelRoot, "typescript/syntax/function", "function foo(): any {}", {
    options: { plugins: ["typescript"] },
    output: {},
  });
  await babelFixture(babelRoot, "optional/syntax/assignment", "(target?.value) = next;", {
    options: { plugins: [["optionalChainingAssign", { version: "2023-07" }]] },
    output: {},
  });
  await write(join(babelRoot, "core/syntax/nested/child"), "input.js", "let value = 1;");
  const babel = await loadBabel(babelRoot);
  assert.deepEqual(babel.inventory, {
    enabledFixtures: 7,
    upstreamDisabled: 1,
    upstreamUndiscovered: 1,
    clean: 4,
    fatal: 1,
    recovery: 2,
    executions: 7,
  });
  assert.equal(babel.extensions.unsupportedReasons["plugin:flow"], 1);
  assert.equal(babel.cases.find((testCase) => testCase.id.includes("typescript/dts"))?.options.lang, "dts");
  assert.equal(babel.cases.find((testCase) => testCase.id.includes("typescript/syntax"))?.options.lang, "ts");
  assert.equal(
    babel.cases.find((testCase) => testCase.id.includes("optional/syntax"))?.options.optionalChainingAssign,
    true,
  );

  const typeScriptRoot = join(root, "typescript", "tests", "cases", "compiler");
  await write(
    typeScriptRoot,
    "multiFile.ts",
    [
      "// @target: es5, esnext",
      "// @filename: a.ts",
      "let value: number = 1;",
      "// @filename: b.ts",
      "const value = ;",
      "// @filename: c.js",
      "function load() { await new Promise(undefined); }",
      "// @filename: tsconfig.json",
      "{}",
    ].join("\n"),
  );
  const typeScript = await loadTypeScript(join(root, "typescript", "tests", "cases"));
  assert.deepEqual(typeScript.inventory, {
    caseFiles: 1,
    configurations: 2,
    sourceUnits: 3,
    nonSourceUnits: 1,
    executions: 6,
    nonSourceExecutions: 2,
  });
  assert.equal(typeScript.cases.filter((testCase) => testCase.expectation === "diagnostic").length, 2);
  assert.equal(
    typeScript.cases.find((testCase) => testCase.path.endsWith(":c.js"))?.options.typescriptJsCompatibility,
    true,
  );
});

async function babelFixture(root, fixture, source, { options, output }) {
  const directory = join(root, fixture);
  await write(directory, "input.js", source);
  if (options) await write(directory, "options.json", JSON.stringify(options));
  if (output) await write(directory, "output.json", JSON.stringify(output));
}

async function write(directory, file, content) {
  await mkdir(directory, { recursive: true });
  await writeFile(join(directory, file), content);
}
