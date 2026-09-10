#!/usr/bin/env node
/** Verify that a built i0i application contains its ARM64 native runtimes. */

import { access, stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";

const REQUIRED = [
  { names: ["pdfium/libpdfium.dylib", "resources/pdfium/libpdfium.dylib"], executable: false },
  { names: ["obscura/obscura", "resources/obscura/obscura"], executable: true },
  { names: ["obscura/obscura-worker", "resources/obscura/obscura-worker"], executable: true },
  { names: ["THIRD_PARTY_NOTICES.md", "resources/THIRD_PARTY_NOTICES.md"], executable: false },
];

/** Run one macOS inspection command. */
function run(program, args) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(program, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolveRun(stdout.trim());
      else reject(new Error(`${basename(program)} failed: ${stderr.trim() || stdout.trim()}`));
    });
  });
}

/** Find a resource in either layout emitted by supported Tauri versions. */
async function findResource(resources, names) {
  for (const name of names) {
    const candidate = join(resources, name);
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Try the next supported bundle layout.
    }
  }
  throw new Error(`missing packaged resource: ${names.join(" or ")}`);
}

/** Verify bundle structure, architecture, permissions, and optional signatures. */
export async function verifyBundle(appPath, verifySignatures = false) {
  const app = resolve(appPath);
  if (!app.endsWith(".app")) throw new Error("bundle path must end in .app");
  const resources = join(app, "Contents", "Resources");
  const executable = join(app, "Contents", "MacOS", "i0i");
  const files = [executable];
  await access(executable);

  for (const required of REQUIRED) {
    const path = await findResource(resources, required.names);
    files.push(path);
    if (required.executable && ((await stat(path)).mode & 0o111) === 0) {
      throw new Error(`packaged executable lacks execute permission: ${path}`);
    }
  }
  for (const path of files.filter((path) => !path.endsWith(".md"))) {
    const architectures = await run("/usr/bin/lipo", ["-archs", path]);
    if (!architectures.split(/\s+/).includes("arm64")) {
      throw new Error(`packaged binary is not ARM64: ${path}`);
    }
  }
  if (verifySignatures) {
    await run("/usr/bin/codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);
  }
  return files;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const appPath = process.argv[2];
  if (!appPath) {
    console.error("Usage: node scripts/verify_macos_bundle.mjs /path/to/i0i.app [--signatures]");
    process.exitCode = 2;
  } else {
    verifyBundle(appPath, process.argv.includes("--signatures"))
      .then((files) => console.log(`Verified ${files.length} bundled files in ${appPath}`))
      .catch((error) => {
        console.error(error.message);
        process.exitCode = 1;
      });
  }
}
