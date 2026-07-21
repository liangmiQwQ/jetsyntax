import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";

const require = createRequire(import.meta.url);
const root = resolve(import.meta.dirname, "../..");
const cacheDirectory = resolve(root, ".cache/fixtures");
const checkerUrl =
  "https://raw.githubusercontent.com/microsoft/TypeScript/c9e7428bb76f0543a3555d0af87777e7db3a41e6/src/compiler/checker.ts";
const typescriptSha256 = "804f9c1b6c64568c39dd48eee88b77ba92d0b5d0f44f425bc96bcfe052824644";
const checkerSha256 = "ffe288edd0eae68f65e4b81b5bbfd4fe5fbed62b55246dc140813078555050fb";
const reactSha256 = "ec670cc82d2aac81844bae49353d11bef1a8a21e727290a3bcc24a2928839496";

export async function prepareFixtures() {
  await mkdir(cacheDirectory, { recursive: true });
  const typescriptPath = join(cacheDirectory, "typescript-5.1.6.js");
  const checkerPath = join(cacheDirectory, "checker-c9e7428.ts");
  const reactPath = join(cacheDirectory, "react-17.0.2.js");
  await copyFile(require.resolve("typescript/lib/typescript.js"), typescriptPath);
  await copyFile(join(dirname(require.resolve("react")), "cjs/react.development.js"), reactPath);

  let checkerSource;
  try {
    checkerSource = await readFile(checkerPath, "utf8");
  } catch {
    const response = await fetch(checkerUrl);
    if (!response.ok) throw new Error(`failed to fetch checker.ts: ${response.status}`);
    checkerSource = await response.text();
    await writeFile(checkerPath, checkerSource);
  }
  verify(checkerSource, checkerSha256, "checker.ts");

  const fixtures = {
    typescript: await fixture(
      typescriptPath,
      "npm:typescript@5.1.6/lib/typescript.js",
      typescriptSha256,
    ),
    checker: await fixture(checkerPath, checkerUrl, checkerSha256),
    react: await fixture(
      reactPath,
      "npm:react@17.0.2/cjs/react.development.js",
      reactSha256,
    ),
  };
  const manifest = Object.fromEntries(
    Object.entries(fixtures).map(([name, value]) => [name, { ...value, source: undefined }]),
  );
  await writeFile(join(cacheDirectory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return fixtures;
}

async function fixture(path, url, sha256) {
  const source = await readFile(path, "utf8");
  verify(source, sha256, url);
  return { path, url, sha256, bytes: Buffer.byteLength(source), source };
}

function verify(source, sha256, name) {
  const actual = digest(source);
  if (actual !== sha256) throw new Error(`${name} checksum mismatch: ${actual}`);
}

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const fixtures = await prepareFixtures();
  for (const [name, value] of Object.entries(fixtures)) {
    console.log(`${name}: ${value.sha256} (${value.bytes} bytes)`);
  }
}
