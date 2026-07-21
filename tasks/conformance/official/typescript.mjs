import { basename, relative } from "node:path";

import ts from "typescript";

import { listFiles, readSource } from "./shared.mjs";

const OPTION_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const SOURCE_EXTENSION = /(?:\.d\.[cm]?ts|\.[cm]?ts|\.tsx|\.[cm]?js|\.jsx)$/i;
const VARY_BY = [
  ...ts.optionDeclarations
    .filter((option) =>
      !option.isCommandLineOnly
      && (option.type === "boolean" || typeof option.type === "object")
      && (option.affectsProgramStructure
        || option.affectsEmit
        || option.affectsModuleResolution
        || option.affectsBindDiagnostics
        || option.affectsSemanticDiagnostics
        || option.affectsSourceFile
        || option.affectsDeclarationPath
        || option.affectsBuildInfo)
    )
    .map((option) => option.name),
  "noEmit",
  "isolatedModules",
];

export async function loadTypeScript(root) {
  const caseFiles = (await listFiles(root)).filter((file) =>
    (file.includes("/compiler/") || file.includes("/conformance/")) && /\.tsx?$/.test(file)
  );
  const cases = [];
  let configurations = 0;
  let sourceUnits = 0;
  let nonSourceUnits = 0;
  let nonSourceExecutions = 0;

  // TypeScript cases are virtual projects. Keep all units from one project on the same shard.
  for (const file of caseFiles) {
    const source = await readSource(file);
    const settings = extractSettings(source);
    const units = splitUnits(source, basename(file));
    const variants = configurationsFor(settings);
    const path = relative(root, file).replaceAll("\\", "/");
    configurations += variants.length;

    for (const unit of units) {
      if (!SOURCE_EXTENSION.test(unit.name)) {
        nonSourceUnits++;
        nonSourceExecutions += variants.length;
        continue;
      }
      sourceUnits++;
      const sourceFile = ts.createSourceFile(
        unit.name,
        unit.content,
        ts.ScriptTarget.Latest,
        true,
        scriptKindFor(unit.name),
      );
      const expectation = sourceFile.parseDiagnostics.length > 0 ? "diagnostic" : "clean";
      for (const variant of variants) {
        const variantSettings = { ...settings, ...variant };
        cases.push({
          id: `${path}#${configurationKey(variant)}#${unit.name}`,
          path: `${path}:${unit.name}`,
          project: path,
          source: unit.content,
          options: {
            lang: languageFor(unit.name),
            preserveParens: true,
            semanticErrors: false,
            sourceType: sourceTypeFor(sourceFile, unit.name, variantSettings),
          },
          expectation,
        });
      }
    }
  }

  return {
    cases,
    inventory: {
      caseFiles: caseFiles.length,
      configurations,
      sourceUnits,
      nonSourceUnits,
      executions: cases.length,
      nonSourceExecutions,
    },
  };
}

function extractSettings(source) {
  const settings = {};
  for (const line of source.split(/\r\n?|\n|\u2028|\u2029/)) {
    const match = OPTION_PATTERN.exec(line);
    if (match) settings[match[1]] = match[2].trim();
  }
  return settings;
}

function splitUnits(source, fallbackName) {
  const units = [];
  let content;
  let name;

  for (const line of source.split(/\r\n?|\n|\u2028|\u2029/)) {
    if (/^\/{2}\s*@link\s*:/i.test(line)) continue;
    const option = OPTION_PATTERN.exec(line);
    if (option) {
      if (option[1].toLowerCase() !== "filename") continue;
      if (name !== undefined) units.push({ name, content: content ?? "" });
      name = option[2].trim();
      content = "";
      continue;
    }
    content = content === undefined || content === "" ? line : `${content}\n${line}`;
  }

  units.push({ name: name ?? fallbackName, content: content ?? "" });
  return units;
}

function configurationsFor(settings) {
  const entries = [];
  for (const option of VARY_BY) {
    if (!Object.hasOwn(settings, option)) continue;
    const values = splitVariation(settings[option], option);
    if (values) entries.push([option, values]);
  }
  if (entries.length === 0) return [{}];

  const configurations = [{}];
  for (const [option, values] of entries) {
    const previous = configurations.splice(0);
    for (const configuration of previous) {
      for (const value of values) configurations.push({ ...configuration, [option]: value });
    }
  }
  if (configurations.length > 25) throw new Error("TypeScript test options exceed 25 variations");
  return configurations;
}

function splitVariation(text, optionName) {
  const includes = [];
  const excludes = [];
  let star = false;
  for (let value of text.split(",")) {
    value = value.trim().toLowerCase();
    if (!value) continue;
    if (value === "*") star = true;
    else if (value.startsWith("-") || value.startsWith("!")) excludes.push(value.slice(1));
    else includes.push(value);
  }
  if (includes.length <= 1 && !star && excludes.length === 0) return undefined;

  const declaration = ts.optionDeclarations.find((option) => option.name.toLowerCase() === optionName.toLowerCase());
  const knownValues = declaration && typeof declaration.type === "object"
    ? declaration.type
    : declaration?.type === "boolean"
    ? new Map([["true", 1], ["false", 0]])
    : undefined;
  const values = [];
  for (const key of includes) addUniqueValue(values, key, knownValues?.get(key));
  if (star && knownValues) {
    for (const [key, value] of knownValues) addUniqueValue(values, key, value);
  }
  for (const key of excludes) {
    const value = knownValues?.get(key);
    for (let index = values.length - 1; index >= 0; index--) {
      if (values[index].key === key || (value !== undefined && values[index].value === value)) values.splice(index, 1);
    }
  }
  if (values.length === 0) throw new Error(`empty TypeScript variation: ${optionName}`);
  return values.map((value) => value.key);
}

function addUniqueValue(values, key, value) {
  if (!values.some((entry) => entry.key === key || (value !== undefined && entry.value === value))) {
    values.push({ key, value });
  }
}

function scriptKindFor(file) {
  const lower = file.toLowerCase();
  if (lower.endsWith(".tsx")) return ts.ScriptKind.TSX;
  if (lower.endsWith(".jsx")) return ts.ScriptKind.JSX;
  if (lower.endsWith(".js") || lower.endsWith(".mjs") || lower.endsWith(".cjs")) return ts.ScriptKind.JS;
  return ts.ScriptKind.TS;
}

function languageFor(file) {
  const lower = file.toLowerCase();
  if (/\.d\.[cm]?ts$/.test(lower)) return "dts";
  if (lower.endsWith(".tsx")) return "tsx";
  if (lower.endsWith(".jsx")) return "jsx";
  if (/\.[cm]?ts$/.test(lower)) return "ts";
  return "js";
}

function sourceTypeFor(sourceFile, file, settings) {
  const lower = file.toLowerCase();
  if (lower.endsWith(".mjs") || lower.endsWith(".mts")) return "module";
  if (lower.endsWith(".cjs") || lower.endsWith(".cts")) return "commonjs";
  if (String(settings.moduleDetection).toLowerCase() === "force") return "module";
  return ts.isExternalModule(sourceFile) ? "module" : "script";
}

function configurationKey(configuration) {
  const entries = Object.entries(configuration).sort(([left], [right]) => left.localeCompare(right));
  return entries.length === 0 ? "default" : entries.map(([key, value]) => `${key}=${value}`).join(",");
}
