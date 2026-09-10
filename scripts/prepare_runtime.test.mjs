/** Focused tests for reproducible runtime artifact preparation. */

import assert from "node:assert/strict";
import { chmod, copyFile, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  DEFAULT_MANIFEST,
  TAURI_DIR,
  loadManifest,
  safeMemberPath,
  sha256,
  verifyFile,
} from "./prepare_runtime.mjs";

test("repository manifest declares both native components", async () => {
  const manifest = await loadManifest(DEFAULT_MANIFEST);
  assert.deepEqual(Object.keys(manifest.components).sort(), ["obscura", "pdfium"]);
  for (const component of Object.values(manifest.components)) {
    assert.match(component.archive_sha256, /^[0-9a-f]{64}$/);
  }
});

test("sha256 reports exact file content", async () => {
  const path = join(tmpdir(), `i0i-digest-${process.pid}`);
  await writeFile(path, "i0i");
  assert.equal(await sha256(path), "01c68122eb0cd8eb7a59854c708c3bb8dc48040286d6a45ee02cf3df5fcb528d");
  await rm(path);
});

test("archive members cannot escape extraction root", () => {
  assert.throws(() => safeMemberPath("/tmp/root", "../escape"), /unsafe archive member/);
  assert.throws(() => safeMemberPath("/tmp/root", "/escape"), /unsafe archive member/);
  assert.equal(safeMemberPath("/tmp/root", "nested/file"), "/tmp/root/nested/file");
});

test("verification rejects a wrong digest before installation", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "i0i-runtime-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "artifact");
  await writeFile(path, "not a runtime");
  await assert.rejects(
    verifyFile({ sha256: "0".repeat(64), mode: "0644" }, path),
    /checksum mismatch/,
  );
});

test("verification rejects a non-Mach-O artifact", { skip: process.platform !== "darwin" }, async (context) => {
  const root = await mkdtemp(join(tmpdir(), "i0i-runtime-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "artifact");
  await writeFile(path, "plain text");
  await assert.rejects(
    verifyFile({ sha256: await sha256(path), mode: "0644" }, path),
    /lipo failed/,
  );
});

test("verification rejects an executable with the wrong mode", { skip: process.platform !== "darwin" }, async (context) => {
  const manifest = await loadManifest(DEFAULT_MANIFEST);
  const spec = manifest.components.obscura.files[0];
  const root = await mkdtemp(join(tmpdir(), "i0i-runtime-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "obscura");
  await copyFile(join(TAURI_DIR, spec.destination), path);
  await chmod(path, 0o644);
  await assert.rejects(verifyFile(spec, path), /permission mismatch/);
});

test("verification rejects an incompatible Pdfium symbol set", { skip: process.platform !== "darwin" }, async (context) => {
  const manifest = await loadManifest(DEFAULT_MANIFEST);
  const spec = manifest.components.pdfium.files[0];
  const root = await mkdtemp(join(tmpdir(), "i0i-runtime-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "libpdfium.dylib");
  await copyFile(join(TAURI_DIR, spec.destination), path);
  await chmod(path, 0o644);
  await assert.rejects(
    verifyFile({ ...spec, required_symbols: ["FPDF_i0i_missing_symbol"] }, path),
    /missing required symbols/,
  );
});
