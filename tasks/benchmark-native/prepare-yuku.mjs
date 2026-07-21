import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const buildDirectory = resolve(root, ".cache/yuku-native-build");
const zigVersion = "0.16.0-dev.2368+380ea6fb5";
const zigArchives = {
  arm64: {
    sha256: "bdf27006ceb6b47a8ce1877b2ee8f401d95803258a0c476b1296553b55d6fdac",
    url: `https://zigmirror.hryx.net/zig/zig-aarch64-macos-${zigVersion}.tar.xz`,
  },
  x64: {
    sha256: "bd96d907034165ae31c56f44033e3b823ef400447a65fd478bb74a4463bb7589",
    url: `https://zig.squirl.dev/zig-x86_64-macos-${zigVersion}.tar.xz`,
  },
};

export async function prepareYuku() {
  if (process.platform !== "darwin" || !(process.arch in zigArchives)) {
    throw new Error(
      `the pinned native Yuku benchmark supports macOS arm64/x64, got ${process.platform} ${process.arch}`,
    );
  }

  const yukuDirectory = resolve(
    process.env.YUKU_DIR ?? resolve(root, "../../yuku-toolchain/yuku"),
  );
  await verifyYukuCheckout(yukuDirectory);
  const zig = await installZig();
  const target = process.arch === "arm64" ? "aarch64-macos.15.0" : "x86_64-macos.15.0";

  // Direct module compilation avoids fetching Yuku's unrelated NAPI build dependency.
  await mkdir(buildDirectory, { recursive: true });
  const binary = resolve(buildDirectory, "yuku-native-benchmark");
  run(zig, [
    "build-exe",
    "-OReleaseFast",
    "-target",
    target,
    "-mcpu=native",
    "-lc",
    "--dep",
    "yuku_parser",
    `-Mroot=${resolve(import.meta.dirname, "yuku/src/main.zig")}`,
    "-OReleaseFast",
    "--dep",
    "util",
    "--dep",
    "codegen_options",
    `-Myuku_parser=${resolve(yukuDirectory, "src/parser/root.zig")}`,
    "-OReleaseFast",
    `-Mutil=${resolve(yukuDirectory, "src/util/root.zig")}`,
    `-Mcodegen_options=${resolve(import.meta.dirname, "yuku/src/codegen_options.zig")}`,
    `-femit-bin=${binary}`,
  ]);

  return {
    binary,
    commit: git(yukuDirectory, ["rev-parse", "HEAD"]),
    directory: yukuDirectory,
    flags: `-OReleaseFast -target ${target} -mcpu=native`,
    zigVersion,
  };
}

async function installZig() {
  const zigDirectory = resolve(root, `.cache/zig-${zigVersion}-${process.arch}`);
  const zig = resolve(zigDirectory, "zig");
  if (await exists(zig)) return zig;

  const archive = zigArchives[process.arch];
  const archivePath = resolve(root, `.cache/zig-${zigVersion}-${process.arch}.tar.xz`);
  await mkdir(resolve(root, ".cache"), { recursive: true });
  run("curl", ["--fail", "--location", "--silent", "--show-error", archive.url, "--output", archivePath]);
  const digest = createHash("sha256").update(await readFile(archivePath)).digest("hex");
  if (digest !== archive.sha256) throw new Error(`Zig archive checksum mismatch: ${digest}`);
  await mkdir(zigDirectory, { recursive: true });
  run("tar", ["-xJf", archivePath, "-C", zigDirectory, "--strip-components=1"]);
  return zig;
}

async function verifyYukuCheckout(directory) {
  const packageManifest = resolve(directory, "build.zig.zon");
  if (!(await exists(packageManifest))) throw new Error(`Yuku checkout not found: ${directory}`);
  const status = git(directory, ["status", "--porcelain"]);
  if (status) throw new Error(`Yuku checkout must be clean: ${directory}`);
}

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

function git(directory, arguments_) {
  return run("git", ["-C", directory, ...arguments_]).trim();
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, { encoding: "utf8", stdio: "pipe", ...options });
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.status}):\n${result.stderr || result.stdout}`);
  }
  return result.stdout;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const yuku = await prepareYuku();
  console.log(`Yuku ${yuku.commit} built with Zig ${yuku.zigVersion}`);
}
