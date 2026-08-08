import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseInline, parseMarkdown, type Block, type Inline } from './markdown.ts';

function text(spans: Inline[]): string {
  return spans
    .map((span) => (span.kind === "cite" ? `[${span.handle}]` : span.text))
    .join("");
}

function kinds(blocks: Block[]): string[] {
  return blocks.map((block) => block.kind);
}

test("plain prose is one paragraph", () => {
  const blocks = parseMarkdown("Attention weights the inputs.");
  assert.deepEqual(kinds(blocks), ["p"]);
  assert.equal(text((blocks[0] as { spans: Inline[] }).spans), "Attention weights the inputs.");
});

test("a blank line separates paragraphs, a single newline does not", () => {
  const blocks = parseMarkdown("first line\nstill first\n\nsecond");
  assert.deepEqual(kinds(blocks), ["p", "p"]);
  assert.equal(text((blocks[0] as { spans: Inline[] }).spans), "first line still first");
});

test("headings carry their level", () => {
  const blocks = parseMarkdown("### Results");
  assert.deepEqual(kinds(blocks), ["h"]);
  assert.equal((blocks[0] as { level: number }).level, 3);
});

test("bullet and numbered lists group their items", () => {
  const bullets = parseMarkdown("- one\n- two\n- three");
  assert.deepEqual(kinds(bullets), ["list"]);
  const list = bullets[0] as { ordered: boolean; items: Inline[][] };
  assert.equal(list.ordered, false);
  assert.deepEqual(list.items.map(text), ["one", "two", "three"]);

  const numbered = parseMarkdown("1. first\n2. second");
  const ordered = numbered[0] as { ordered: boolean; items: Inline[][] };
  assert.equal(ordered.ordered, true);
  assert.deepEqual(ordered.items.map(text), ["first", "second"]);
});

test("a list ends at the paragraph after it", () => {
  const blocks = parseMarkdown("- one\n- two\n\nAnd so on.");
  assert.deepEqual(kinds(blocks), ["list", "p"]);
});

test("fenced code keeps its lines verbatim", () => {
  const blocks = parseMarkdown("before\n\n```python\nx = 1\ny = 2\n```\n\nafter");
  assert.deepEqual(kinds(blocks), ["p", "code", "p"]);
  assert.equal((blocks[1] as { text: string }).text, "x = 1\ny = 2");
});

test("an unterminated fence runs to the end rather than leaking backticks", () => {
  // Models truncate mid-block; swallowing the rest beats rendering stray ```.
  const blocks = parseMarkdown("```\nx = 1");
  assert.deepEqual(kinds(blocks), ["code"]);
  assert.equal((blocks[0] as { text: string }).text, "x = 1");
});

test("markdown inside a code fence is not parsed", () => {
  const blocks = parseMarkdown("```\n- not a list\n**not bold**\n```");
  assert.deepEqual(kinds(blocks), ["code"]);
  assert.equal((blocks[0] as { text: string }).text, "- not a list\n**not bold**");
});

test("block quotes and rules are their own blocks", () => {
  const blocks = parseMarkdown("> quoted line\n> and more\n\n---\n\ntail");
  assert.deepEqual(kinds(blocks), ["quote", "hr", "p"]);
  assert.equal((blocks[0] as { spans: Inline[] }).spans.map((s) => text([s])).join(""), "quoted line and more");
});

test("inline code, bold and italic become their own spans", () => {
  const spans = parseInline("set `d_k` to **512**, roughly *half*");
  assert.deepEqual(
    spans.map((span) => span.kind),
    ["text", "code", "text", "strong", "text", "em"],
  );
  assert.equal(spans[1].kind === "code" && spans[1].text, "d_k");
  assert.equal(spans[3].kind === "strong" && spans[3].text, "512");
});

test("underscores inside identifiers are left alone", () => {
  // `snake_case_name` is far more common here than underscore-italics.
  const spans = parseInline("the value of d_k in scaled_dot_product");
  assert.deepEqual(spans.map((span) => span.kind), ["text"]);
});

test("citation markers survive as their own spans", () => {
  const spans = parseInline("They divide by sqrt(d_k) [1] to keep gradients stable.");
  assert.deepEqual(
    spans.map((span) => span.kind),
    ["text", "cite", "text"],
  );
  assert.equal(spans[1].kind === "cite" && spans[1].handle, "1");
});

test("a citation inside a list item still parses", () => {
  const blocks = parseMarkdown("- scaling matters [2]\n- softmax saturates");
  const list = blocks[0] as { items: Inline[][] };
  assert.equal(list.items[0].some((span) => span.kind === "cite"), true);
});

test("an unmatched asterisk stays literal", () => {
  // A wrong render is worse than an unrendered asterisk.
  const spans = parseInline("2 * 3 = 6");
  assert.deepEqual(spans.map((span) => span.kind), ["text"]);
  assert.equal(text(spans), "2 * 3 = 6");
});

test("a partially streamed bold does not swallow the rest", () => {
  // Mid-stream the closing ** has not arrived yet.
  const spans = parseInline("this is **half writ");
  assert.equal(text(spans), "this is **half writ");
});
