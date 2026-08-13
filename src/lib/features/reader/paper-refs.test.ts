import { test } from "node:test";
import assert from "node:assert/strict";
import { citeKey, refSlug, resolveRef, shortTitle, type RefIndex } from "./paper-refs.ts";

function index(): RefIndex {
  return {
    papers: [
      {
        paperId: "p1",
        vaultId: "v1",
        vaultSlug: "transformers",
        citationKey: "vaswani-2017",
        title: "Attention Is All You Need",
        year: 2017,
      },
      {
        paperId: "p2",
        vaultId: "v2",
        vaultSlug: "interpretability",
        citationKey: "vaswani-2017",
        title: "Attention Is All You Need",
        year: 2017,
      },
      {
        paperId: "p3",
        vaultId: "v2",
        vaultSlug: "interpretability",
        citationKey: "elhage-2021",
        title: "A Mathematical Framework for Transformer Circuits",
        year: 2021,
      },
    ],
    vaults: new Map([
      ["transformers", "v1"],
      ["interpretability", "v2"],
    ]),
  };
}

test("a qualified reference resolves to the paper in that vault", () => {
  const resolved = resolveRef(index(), { vault: "interpretability", key: "vaswani-2017" }, "v1");
  assert.equal(resolved?.kind, "paper");
  assert.equal(resolved?.kind === "paper" && resolved.paperId, "p2");
});

test("an unqualified reference prefers the current vault", () => {
  const resolved = resolveRef(index(), { key: "vaswani-2017" }, "v2");
  assert.equal(resolved?.kind === "paper" && resolved.paperId, "p2");
});

test("an ambiguous unqualified reference outside both vaults does not guess", () => {
  assert.equal(resolveRef(index(), { key: "vaswani-2017" }, "v9"), null);
});

test("an unambiguous unqualified reference resolves from any vault", () => {
  const resolved = resolveRef(index(), { key: "elhage-2021" }, "v1");
  assert.equal(resolved?.kind === "paper" && resolved.paperId, "p3");
});

test("an unknown key resolves to nothing", () => {
  assert.equal(resolveRef(index(), { vault: "transformers", key: "nope-1999" }, "v1"), null);
});

test("a known key in the wrong vault resolves to nothing", () => {
  assert.equal(resolveRef(index(), { vault: "transformers", key: "elhage-2021" }, "v1"), null);
});

test("the vault form resolves to the vault", () => {
  const resolved = resolveRef(index(), { vault: "transformers", key: "" }, "v2");
  assert.equal(resolved?.kind, "vault");
  assert.equal(resolved?.kind === "vault" && resolved.vaultId, "v1");
});

test("slugs fold case and separators", () => {
  assert.equal(refSlug("  Transformers/ "), "transformers");
  assert.equal(refSlug("Vaswani 2017"), "vaswani-2017");
});

test("short titles cut at the first clause and cap length", () => {
  assert.equal(shortTitle("Attention Is All You Need"), "Attention Is All You Need");
  assert.equal(
    shortTitle("A Mathematical Framework for Transformer Circuits: Part Two"),
    "A Mathematical Framework for Transformer…",
  );
});

// The key mirrors the BibTeX export's, so a note and a bibliography agree.
test("cite keys are family + year + first significant title word", () => {
  assert.equal(
    citeKey({ authors: ["Ashish Vaswani", "Noam Shazeer"], year: 2017, title: "Attention Is All You Need" }),
    "vaswani2017attention",
  );
});

test("cite keys handle Family, Given and missing metadata", () => {
  assert.equal(citeKey({ authors: ["Elhage, Nelson"], year: 2021, title: "A Mathematical Framework" }), "elhage2021mathematical");
  assert.equal(citeKey({ authors: [], year: 0, title: "The Of On" }), "anonnd");
});
