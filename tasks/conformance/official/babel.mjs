import { readdir } from "node:fs/promises";
import { basename, extname, join, relative } from "node:path";

import { listFiles, readJsonOptional, readSource } from "./shared.mjs";

const INPUT_PATTERN = /^input\.(?:js|mjs|ts|tsx|cts|mts|vue)$/;
const NON_DIALECT_PLUGINS = new Set([
  "decorators-legacy",
  "flow",
  "flowComments",
  "placeholders",
  "recordAndTuple",
  "v8intrinsic",
]);
const UNSUPPORTED_OPTIONS = new Set([
  "allowAwaitOutsideFunction",
  "allowNewTargetOutsideFunction",
  "allowSuperOutsideMethod",
  "allowUndeclaredExports",
  "allowYieldOutsideFunction",
]);

export async function loadBabel(root) {
  const allFiles = await listFiles(root);
  const allInputs = allFiles.filter((file) => INPUT_PATTERN.test(basename(file)));
  const cases = [];
  const plugins = {};
  const unsupportedReasons = {};
  const expectations = { clean: 0, fatal: 0, recovery: 0 };
  let upstreamDisabled = 0;
  let discoveredInputs = 0;

  // Babel's helper-fixtures package has a fixed category/suite/task hierarchy.
  for (const category of await directoryNames(root)) {
    const categoryRoot = join(root, category);
    const categoryOptions = await readJsonOptional(join(categoryRoot, "options.json")) ?? {};
    for (const suite of await directoryNames(categoryRoot)) {
      const suiteRoot = join(categoryRoot, suite);
      const suiteOptions = await readJsonOptional(join(suiteRoot, "options.json")) ?? categoryOptions;
      for (const task of await directoryNames(suiteRoot, false)) {
        const taskRoot = join(suiteRoot, task);
        const input = (await fileNames(taskRoot)).find((file) => INPUT_PATTERN.test(file));
        if (!input) continue;
        discoveredInputs++;

        const options = {
          ...structuredClone(suiteOptions),
          ...(await readJsonOptional(join(taskRoot, "options.json")) ?? {}),
        };
        if (task.startsWith(".") || options.BABEL_8_BREAKING === false) {
          upstreamDisabled++;
          continue;
        }

        const output = await readJsonOptional(join(taskRoot, "output.json"))
          ?? await readJsonOptional(join(taskRoot, "output.extended.json"));
        const expectation = options.throws
          ? "fatal"
          : output?.errors?.length > 0
          ? "recovery"
          : output
          ? "clean"
          : undefined;
        if (!expectation) throw new Error(`Babel fixture has no expectation: ${taskRoot}`);
        expectations[expectation]++;

        const pluginNames = (options.plugins ?? []).map((plugin) => String(Array.isArray(plugin) ? plugin[0] : plugin));
        for (const plugin of pluginNames) plugins[plugin] = (plugins[plugin] ?? 0) + 1;
        const reasons = extensionReasons(input, options, pluginNames);
        for (const reason of reasons) unsupportedReasons[reason] = (unsupportedReasons[reason] ?? 0) + 1;

        const path = relative(root, join(taskRoot, input)).replaceAll("\\", "/");
        cases.push({
          id: path,
          path,
          source: await readSource(join(taskRoot, input)),
          options: {
            allowReturnOutsideFunction: options.allowReturnOutsideFunction === true,
            lang: languageFor(input, options.plugins ?? []),
            preserveParens: true,
            semanticErrors: true,
            sourceType: options.sourceType ?? "script",
          },
          expectation,
          unsupportedReasons: reasons,
        });
      }
    }
  }

  return {
    cases,
    extensions: { plugins, unsupportedReasons },
    inventory: {
      enabledFixtures: cases.length,
      upstreamDisabled,
      upstreamUndiscovered: allInputs.length - discoveredInputs,
      ...expectations,
      executions: cases.length,
    },
  };
}

async function directoryNames(root, ignoreSpecial = true) {
  const entries = await readdir(root, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isDirectory() && (!ignoreSpecial || !entry.name.startsWith(".")))
    .map((entry) => entry.name)
    .sort();
}

async function fileNames(root) {
  const entries = await readdir(root, { withFileTypes: true });
  return entries.filter((entry) => entry.isFile()).map((entry) => entry.name).sort();
}

function extensionReasons(input, options, pluginNames) {
  const reasons = pluginNames.filter((plugin) => NON_DIALECT_PLUGINS.has(plugin)).map((plugin) => `plugin:${plugin}`);
  for (const option of UNSUPPORTED_OPTIONS) {
    if (options[option] === true) reasons.push(`option:${option}`);
  }
  if (extname(input) === ".vue") reasons.push("language:vue");
  return [...new Set(reasons)].sort();
}

function languageFor(input, plugins) {
  const extension = extname(input).toLowerCase();
  const pluginNames = plugins.map((plugin) => String(Array.isArray(plugin) ? plugin[0] : plugin));
  const typescript = pluginNames.includes("typescript") || [".ts", ".tsx", ".mts", ".cts"].includes(extension);
  const dts = plugins.some((plugin) => Array.isArray(plugin) && plugin[0] === "typescript" && plugin[1]?.dts === true);
  const jsx = pluginNames.includes("jsx") || extension === ".jsx" || extension === ".tsx";
  if (dts) return "dts";
  if (typescript) return jsx ? "tsx" : "ts";
  return jsx ? "jsx" : "js";
}
