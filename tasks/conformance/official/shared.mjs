import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

export async function listFiles(root) {
  const entries = await readdir(root, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .sort();
}

export async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

export async function readJsonOptional(file) {
  try {
    return await readJson(file);
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw error;
  }
}

export async function readSource(file) {
  const bytes = await readFile(file);
  if (bytes[0] === 0xFF && bytes[1] === 0xFE) {
    return new TextDecoder("utf-16le").decode(bytes.subarray(2));
  }
  if (bytes[0] === 0xFE && bytes[1] === 0xFF) {
    return new TextDecoder("utf-16be").decode(bytes.subarray(2));
  }
  return bytes.toString("utf8").replace(/^\uFEFF/, "");
}

export function hash(value) {
  let output = 0x81_1C_9D_C5;
  for (let index = 0; index < value.length; index++) {
    output ^= value.charCodeAt(index);
    output = Math.imul(output, 0x01_00_01_93);
  }
  return output >>> 0;
}
