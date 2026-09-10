#!/usr/bin/env node
/** Prepare and verify native runtime artifacts declared by i0i. */

import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { parse } from "smol-toml";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const TAURI_DIR = join(ROOT, "src-tauri");
export const DEFAULT_MANIFEST = join(TAURI_DIR, "runtime-dependencies.toml");
export const NOTICE_PATH = join(TAURI_DIR, "resources", "THIRD_PARTY_NOTICES.md");

/** Return the lowercase SHA-256 digest of a file. */
export async function sha256(path) {
  const digest = createHash("sha256");
  digest.update(await readFile(path));
  return digest.digest("hex");
}

/** Read and minimally validate the runtime dependency manifest. */
export async function loadManifest(path = DEFAULT_MANIFEST) {
  const manifest = parse(await readFile(path, "utf8"));
  if (manifest.target !== "aarch64-apple-darwin") {
    throw new Error("runtime manifest target must be aarch64-apple-darwin");
  }
  if (!manifest.components || Object.keys(manifest.components).length === 0) {
    throw new Error("runtime manifest must declare components");
  }
  for (const [name, component] of Object.entries(manifest.components)) {
    for (const key of ["version", "url", "archive_sha256", "archive_kind", "license"]) {
      if (!component[key]) throw new Error(`component ${name} is missing ${key}`);
    }
    if (!Array.isArray(component.files) || component.files.length === 0) {
      throw new Error(`component ${name} has no files`);
    }
  }
  return manifest;
}

/** Execute a release utility and return its standard output. */
export async function run(program, args) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(program, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolveRun(stdout.trim());
      else reject(new Error(`${program} failed: ${stderr.trim() || stdout.trim() || `exit ${code}`}`));
    });
  });
}

/** Download one immutable archive to disk. */
async function download(url, destination) {
  const response = await fetch(url, { headers: { "user-agent": "i0i-runtime-preparer/0.1" } });
  if (!response.ok) throw new Error(`download failed with HTTP ${response.status}: ${url}`);
  await writeFile(destination, Buffer.from(await response.arrayBuffer()));
}

/** Ensure a manifest member cannot escape the temporary extraction directory. */
export function safeMemberPath(root, member) {
  if (isAbsolute(member)) throw new Error(`unsafe archive member: ${member}`);
  const path = resolve(root, member);
  if (relative(root, path).startsWith("..")) throw new Error(`unsafe archive member: ${member}`);
  return path;
}

/** Extract a trusted, checksum-verified release archive. */
async function extractArchive(archive, kind, destination) {
  await mkdir(destination, { recursive: true });
  if (kind === "zip") {
    await run("/usr/bin/ditto", ["-x", "-k", archive, destination]);
  } else if (kind === "tar.gz") {
    await run("/usr/bin/tar", ["-xzf", archive, "-C", destination]);
  } else {
    throw new Error(`unsupported archive kind: ${kind}`);
  }
}

/** Require a thin or universal Mach-O containing ARM64. */
async function verifyArchitecture(path) {
  const architectures = await run("/usr/bin/lipo", ["-archs", path]);
  if (!architectures.split(/\s+/).includes("arm64")) {
    throw new Error(`${path} is not an ARM64 Mach-O: ${architectures}`);
  }
}

/** Require each declared exported symbol in a dynamic library. */
async function verifySymbols(path, symbols = []) {
  if (symbols.length === 0) return;
  const exports = await run("/usr/bin/nm", ["-gU", path]);
  const missing = symbols.filter((symbol) => !exports.includes(`_${symbol}`));
  if (missing.length > 0) throw new Error(`${path} is missing required symbols: ${missing.join(", ")}`);
}

/** Verify digest, architecture, permissions, and declared symbols. */
export async function verifyFile(spec, path) {
  const metadata = await stat(path).catch(() => null);
  if (!metadata?.isFile()) throw new Error(`runtime artifact is missing: ${path}`);
  const actualDigest = await sha256(path);
  if (actualDigest !== spec.sha256) {
    throw new Error(`checksum mismatch for ${path}: expected ${spec.sha256}, got ${actualDigest}`);
  }
  await verifyArchitecture(path);
  const expectedMode = Number.parseInt(spec.mode, 8);
  const actualMode = metadata.mode & 0o777;
  if (actualMode !== expectedMode) {
    throw new Error(`permission mismatch for ${path}: expected ${spec.mode}, got ${actualMode.toString(8).padStart(4, "0")}`);
  }
  await verifySymbols(path, spec.required_symbols);
}

/** Return whether every installed component file passes verification. */
async function componentReady(component) {
  try {
    await Promise.all(component.files.map((spec) => verifyFile(spec, join(TAURI_DIR, spec.destination))));
    return true;
  } catch {
    return false;
  }
}

/** Install one component atomically, or verify its installed files. */
async function prepareComponent(name, component, verifyOnly) {
  if (await componentReady(component)) {
    console.log(`${name} ${component.version}: ready`);
    return;
  }
  if (verifyOnly) {
    for (const spec of component.files) await verifyFile(spec, join(TAURI_DIR, spec.destination));
    return;
  }

  const temporaryRoot = await mkdtemp(join(tmpdir(), `i0i-${name}-`));
  try {
    const archive = join(temporaryRoot, "artifact");
    const extracted = join(temporaryRoot, "extracted");
    console.log(`${name} ${component.version}: downloading ${component.url}`);
    await download(component.url, archive);
    const archiveDigest = await sha256(archive);
    if (archiveDigest !== component.archive_sha256) {
      throw new Error(`archive checksum mismatch for ${name}: expected ${component.archive_sha256}, got ${archiveDigest}`);
    }
    await extractArchive(archive, component.archive_kind, extracted);
    for (const spec of component.files) {
      const source = safeMemberPath(extracted, spec.source);
      const destination = join(TAURI_DIR, spec.destination);
      const temporary = `${destination}.new`;
      await mkdir(dirname(destination), { recursive: true });
      await copyFile(source, temporary);
      await chmod(temporary, Number.parseInt(spec.mode, 8));
      await verifyFile(spec, temporary);
      await rename(temporary, destination);
    }
    console.log(`${name} ${component.version}: installed`);
  } finally {
    await rm(temporaryRoot, { recursive: true, force: true });
  }
}

/** Generate the concise native-runtime notice bundled with the app. */
export async function writeNotices(manifest) {
  const lines = ["# Third-Party Native Runtime Notices", "", "Generated from `src-tauri/runtime-dependencies.toml`.", ""];
  for (const [name, component] of Object.entries(manifest.components)) {
    lines.push(`## ${name}`, "", `- Version: ${component.version}`, `- License: ${component.license}`, `- Project: ${component.project_url}`, "");
  }
  await mkdir(dirname(NOTICE_PATH), { recursive: true });
  await writeFile(NOTICE_PATH, `${lines.join("\n").trimEnd()}\n`, "utf8");
}

/** Parse the intentionally small command-line interface. */
function parseArgs(argv) {
  const result = { components: [], verifyOnly: false, manifest: DEFAULT_MANIFEST };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--verify") result.verifyOnly = true;
    else if (argv[index] === "--component") result.components.push(argv[++index]);
    else if (argv[index] === "--manifest") result.manifest = resolve(argv[++index]);
    else throw new Error(`unknown argument: ${argv[index]}`);
  }
  return result;
}

/** Prepare selected runtime components and generate release notices. */
export async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const manifest = await loadManifest(args.manifest);
  const selected = new Set(args.components.length > 0 ? args.components : Object.keys(manifest.components));
  const unknown = [...selected].filter((name) => !manifest.components[name]);
  if (unknown.length > 0) throw new Error(`unknown runtime components: ${unknown.join(", ")}`);
  for (const [name, component] of Object.entries(manifest.components)) {
    if (selected.has(name)) await prepareComponent(name, component, args.verifyOnly);
  }
  await writeNotices(manifest);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
