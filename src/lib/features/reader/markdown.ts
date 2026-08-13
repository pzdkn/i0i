/**
 * A small markdown subset for chat answers.
 *
 * Deliberately not a markdown library. Two reasons:
 *
 * - **No HTML.** This produces a tree of typed nodes that Svelte renders as
 *   text, so model output can never become markup. A library would hand back an
 *   HTML string and `{@html}` would make every answer an injection surface.
 * - **Citations are inline nodes.** `[1]` markers have to survive parsing and
 *   come out as clickable spans, which means the inline pass has to know about
 *   them (RFC 0078).
 *
 * The subset is what models actually emit in short answers: paragraphs,
 * headings, bullet and numbered lists, fenced code, block quotes, rules, and
 * inline code/bold/italic. Anything else stays literal — a wrong render is
 * worse than an unrendered asterisk.
 */

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string }
  /** A `[1]`-style citation marker; the caller resolves the handle. */
  | { kind: "cite"; handle: string }
  /**
   * A `[@vault/citation-key]` reference to a paper in the library (RFC 0090).
   *
   * Parsed, not resolved: `vault` is undefined for the unqualified
   * `[@citation-key]` form, and whether either half names anything real is the
   * caller's question. An unresolvable reference renders as the literal text it
   * was written as — same rule as a `[n]` the assembly never minted.
   */
  | { kind: "paperRef"; vault?: string; key: string; raw: string };

export type Block =
  | { kind: "p"; spans: Inline[] }
  | { kind: "h"; level: number; spans: Inline[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  /** `lang` is the fence tag, when the model gave one. */
  | { kind: "code"; text: string; lang: string }
  | { kind: "quote"; spans: Inline[] }
  | { kind: "hr" };

const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;
const RULE = /^\s*([-*_])\1{2,}\s*$/;
const FENCE = /^\s*```(.*)$/;

export function parseMarkdown(body: string): Block[] {
  const lines = body.split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length) {
      blocks.push({ kind: "p", spans: parseInline(paragraph.join(" ")) });
      paragraph = [];
    }
  };

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];

    const fence = line.match(FENCE);
    if (fence) {
      flushParagraph();
      const lang = fence[1].trim();
      const code: string[] = [];
      index += 1;
      // An unterminated fence runs to the end — models truncate mid-block, and
      // swallowing the rest is better than rendering stray backticks.
      while (index < lines.length && !FENCE.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      // Trailing blank lines are the model's formatting, not content.
      while (code.length && !code[code.length - 1].trim()) {
        code.pop();
      }
      blocks.push({ kind: "code", text: code.join("\n"), lang });
      continue;
    }

    if (!line.trim()) {
      flushParagraph();
      continue;
    }

    if (RULE.test(line)) {
      flushParagraph();
      blocks.push({ kind: "hr" });
      continue;
    }

    const heading = line.match(HEADING);
    if (heading) {
      flushParagraph();
      blocks.push({
        kind: "h",
        level: heading[1].length,
        spans: parseInline(heading[2]),
      });
      continue;
    }

    const bullet = line.match(BULLET);
    const numbered = line.match(NUMBERED);
    if (bullet || numbered) {
      flushParagraph();
      const ordered = Boolean(numbered);
      const items: Inline[][] = [];
      while (index < lines.length) {
        const item = lines[index].match(ordered ? NUMBERED : BULLET);
        if (!item) {
          break;
        }
        items.push(parseInline(item[1]));
        index += 1;
      }
      index -= 1;
      blocks.push({ kind: "list", ordered, items });
      continue;
    }

    const quote = line.match(QUOTE);
    if (quote) {
      flushParagraph();
      const quoted: string[] = [quote[1]];
      while (index + 1 < lines.length) {
        const next = lines[index + 1].match(QUOTE);
        if (!next) {
          break;
        }
        quoted.push(next[1]);
        index += 1;
      }
      blocks.push({ kind: "quote", spans: parseInline(quoted.join(" ")) });
      continue;
    }

    paragraph.push(line.trim());
  }

  flushParagraph();
  return blocks;
}

/**
 * Inline code, bold, italic, and citation markers.
 *
 * One pass, first match wins, no nesting: `**bold *and* italic**` renders bold
 * throughout rather than guessing. Emphasis inside a word (`d_k`, `snake_case`)
 * is deliberately not matched — underscores in identifiers are far more common
 * in this app than underscore-italics.
 */
export function parseInline(text: string): Inline[] {
  const spans: Inline[] = [];
  let plain = "";

  const pushPlain = () => {
    if (plain) {
      spans.push({ kind: "text", text: plain });
      plain = "";
    }
  };

  let index = 0;
  while (index < text.length) {
    const rest = text.slice(index);

    const code = rest.match(/^`([^`\n]+)`/);
    if (code) {
      pushPlain();
      spans.push({ kind: "code", text: code[1] });
      index += code[0].length;
      continue;
    }

    const strong = rest.match(/^\*\*([^\n]+?)\*\*/);
    if (strong) {
      pushPlain();
      spans.push({ kind: "strong", text: strong[1] });
      index += strong[0].length;
      continue;
    }

    const em = rest.match(/^\*([^*\n]+?)\*/);
    if (em) {
      pushPlain();
      spans.push({ kind: "em", text: em[1] });
      index += em[0].length;
      continue;
    }

    // RFC 0090: before the citation rule, since `[@…]` can never be `[1]` but
    // sharing the opening bracket makes the ordering worth being explicit about.
    const paperRef = rest.match(/^\[@([A-Za-z0-9._-]+)?(?:\/([A-Za-z0-9._-]*))?\]/);
    if (paperRef) {
      pushPlain();
      const [raw, first, second] = paperRef;
      // `[@vault/key]` gives both; `[@key]` gives only the first; `[@vault/]`
      // gives a vault and an empty key, which is a reference to the vault.
      const vault = second === undefined ? undefined : first;
      const key = second === undefined ? (first ?? "") : second;
      if (!vault && !key) {
        plain += text[index];
        index += 1;
        continue;
      }
      spans.push({ kind: "paperRef", vault, key, raw });
      index += raw.length;
      continue;
    }

    const cite = rest.match(/^\[(\d+)\]/);
    if (cite) {
      pushPlain();
      spans.push({ kind: "cite", handle: cite[1] });
      index += cite[0].length;
      continue;
    }

    plain += text[index];
    index += 1;
  }

  pushPlain();
  return spans;
}
