import { relative } from "node:path";

import { parse } from "yaml";

import { listFiles, readSource } from "./shared.mjs";

const STRICT_PREFIX = "\"use strict\";\n";

export async function loadTest262(root) {
  const allFiles = await listFiles(root);
  const fixtureFiles = allFiles.filter((file) => file.endsWith(".js") && file.includes("_FIXTURE"));
  const sourceFiles = allFiles.filter((file) => file.endsWith(".js") && !file.includes("_FIXTURE"));
  const cases = [];
  let parseNegativeFiles = 0;

  // Test262 metadata controls both source mode and whether early errors are expected.
  for (const file of sourceFiles) {
    const source = await readSource(file);
    const metadata = parseMetadata(source, file);
    const flags = new Set(metadata.flags ?? []);
    const phase = metadata.negative?.phase;
    const expectation = phase === "parse" || phase === "early" ? "diagnostic" : "clean";
    if (expectation === "diagnostic") parseNegativeFiles++;

    const path = relative(root, file).replaceAll("\\", "/");
    for (const variant of variantsFor(flags)) {
      cases.push({
        id: `${path}#${variant.name}`,
        path,
        source: variant.strict ? STRICT_PREFIX + source : source,
        options: {
          lang: "js",
          preserveParens: true,
          semanticErrors: true,
          sourceType: variant.sourceType,
        },
        expectation,
      });
    }
  }

  return {
    cases,
    inventory: {
      standaloneFiles: sourceFiles.length,
      fixtureFiles: fixtureFiles.length,
      executions: cases.length,
      parseNegativeFiles,
    },
  };
}

function parseMetadata(source, file) {
  const match = /\/\*---([\s\S]*?)---\*\//.exec(source);
  if (!match) throw new Error(`missing Test262 metadata: ${file}`);
  const yaml = match[1].replaceAll("\r\n", "\n").replaceAll("\r", "\n");
  const metadata = parse(yaml);
  if (!metadata || typeof metadata !== "object") throw new Error(`invalid Test262 metadata: ${file}`);
  return metadata;
}

function variantsFor(flags) {
  if (flags.has("module")) return [{ name: "module", sourceType: "module", strict: false }];
  if (flags.has("raw")) return [{ name: "raw", sourceType: "script", strict: false }];
  if (flags.has("noStrict")) return [{ name: "sloppy", sourceType: "script", strict: false }];
  if (flags.has("onlyStrict")) return [{ name: "strict", sourceType: "script", strict: true }];
  return [
    { name: "sloppy", sourceType: "script", strict: false },
    { name: "strict", sourceType: "script", strict: true },
  ];
}
