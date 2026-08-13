/**
 * Resolving `[@vault/citation-key]` references in notes (RFC 0090).
 *
 * The parser (`markdown.ts`) turns the notation into a `paperRef` node without
 * asking whether it names anything. This module answers that, against an index
 * the caller builds from the library it already holds — no round trip.
 *
 * The rule that matters: **an unresolvable reference is not a link.** It renders
 * as the literal text that was typed, the same way `markdown.ts` treats a `[n]`
 * the assembly never minted. A reference that scrolls nowhere is worse than a
 * bracket that was never a reference.
 */

/**
 * The cite key for a paper, mirroring the BibTeX export's
 * `cite_key_base` (`src-tauri/src/services/bibtex.rs:82`):
 * `<family><year><first significant title word>`, ASCII-folded and lowercased.
 *
 * This mirrors deliberately rather than reading `ReaderDocument.citationKey`,
 * which is the *paper id* (`reader_service.rs:592`) — `paper_1786…` is not
 * something anyone types into a note. The key you would cite this paper by in a
 * bibliography is the key you write in a note, and the two agree because they
 * are computed the same way.
 */
const TITLE_STOPWORDS = new Set(["a", "an", "the", "of", "on", "in", "for", "and", "to"]);

function asciiKey(value: string): string {
  return value.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
}

function familyName(name: string): string {
  const trimmed = name.trim();
  const comma = trimmed.indexOf(",");
  if (comma !== -1) {
    return trimmed.slice(0, comma).trim();
  }
  const space = trimmed.lastIndexOf(" ");
  return space === -1 ? trimmed : trimmed.slice(space + 1);
}

function firstSignificantTitleWord(title: string): string {
  for (const raw of title.split(/\s+/)) {
    const word = asciiKey(raw);
    if (word && !TITLE_STOPWORDS.has(word)) {
      return word;
    }
  }
  return "";
}

export function citeKey(paper: { authors: string[]; year: number; title: string }): string {
  const author = asciiKey(familyName(paper.authors[0] ?? "")) || "anon";
  const year = paper.year > 0 ? String(paper.year) : "nd";
  return `${author}${year}${firstSignificantTitleWord(paper.title)}`;
}

/** One paper as the reference index sees it. */
export type RefPaper = {
  paperId: string;
  vaultId: string;
  /** Slug of the vault's path/title, as written after the `@`. */
  vaultSlug: string;
  citationKey: string;
  title: string;
  year: number;
};

export type RefIndex = {
  papers: RefPaper[];
  /** Vault slug → id, for the `[@vault/]` form. */
  vaults: Map<string, string>;
};

export type ResolvedRef =
  | { kind: "paper"; paperId: string; vaultId: string; label: string; title: string }
  | { kind: "vault"; vaultId: string; label: string; title: string };

/**
 * A vault or citation key as it is written inside `[@…]`: lowercase, with runs
 * of anything that is not a letter, digit, dot or dash collapsed to a dash.
 */
export function refSlug(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/^\/+|\/+$/g, "")
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/**
 * Resolve one reference.
 *
 * `vault` undefined is the unqualified `[@key]` form: it resolves within
 * `currentVaultId` when that vault has the key, and otherwise not at all.
 * Guessing between two papers with the same key in different vaults would link
 * to the wrong one silently, so ambiguity resolves to `null`.
 */
export function resolveRef(
  index: RefIndex,
  ref: { vault?: string; key: string },
  currentVaultId: string,
): ResolvedRef | null {
  const key = refSlug(ref.key);
  const vaultSlug = ref.vault === undefined ? undefined : refSlug(ref.vault);

  // `[@vault/]` — the vault itself.
  if (vaultSlug !== undefined && key === "") {
    const vaultId = index.vaults.get(vaultSlug);
    return vaultId ? { kind: "vault", vaultId, label: vaultSlug, title: vaultSlug } : null;
  }

  if (key === "") {
    return null;
  }

  const matches = index.papers.filter((paper) => refSlug(paper.citationKey) === key);
  if (matches.length === 0) {
    return null;
  }

  const paper =
    vaultSlug !== undefined
      ? matches.find((candidate) => candidate.vaultSlug === vaultSlug)
      : (matches.find((candidate) => candidate.vaultId === currentVaultId) ??
        (matches.length === 1 ? matches[0] : undefined));

  if (!paper) {
    return null;
  }

  return {
    kind: "paper",
    paperId: paper.paperId,
    vaultId: paper.vaultId,
    label: shortTitle(paper.title) || paper.citationKey,
    title: paper.year ? `${paper.title} (${paper.year})` : paper.title,
  };
}

/**
 * A title short enough to sit inside a sentence. Prose reads better with names
 * than with slugs (RFC 0090 Open Decision A), but a full paper title inside a
 * note is a wall — the first clause, capped.
 */
export function shortTitle(title: string): string {
  const trimmed = title.trim();
  if (!trimmed) {
    return "";
  }
  const clause = trimmed.split(/[:—–]/)[0].trim();
  const source = clause.length >= 12 ? clause : trimmed;
  return source.length <= 42 ? source : `${source.slice(0, 41).trimEnd()}…`;
}
