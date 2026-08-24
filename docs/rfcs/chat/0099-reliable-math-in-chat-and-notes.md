# RFC 0099: Reliable Math In Chat And Notes

Status: Implemented (manual packaged-app verification pending)
Date: 2026-08-24
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0078 (typed chat rendering), RFC 0090 (paper references in notes)

## Summary

i0i does not currently render mathematical notation. Its small typed Markdown
parser has no math nodes, so formulas are ordinary text, may collide with
emphasis syntax, and lose multiline layout.

Extend the existing safe typed parser with inline and display math nodes. Render
them with locally bundled KaTeX while preserving literal readable input when an
expression is incomplete, malformed, or unsupported.

## Decision

### 1. Keep the typed Markdown boundary

Do not replace the parser and do not pass model output to `{@html}`. Add:

```ts
type Inline =
  | ExistingInline
  | { kind: "math"; tex: string; raw: string };

type Block =
  | ExistingBlock
  | { kind: "math"; tex: string; raw: string };
```

Chat answers and notes share these nodes and one rendering component.

### 2. Define one canonical syntax

Models are instructed to emit:

- inline math as `\(x^2 + y^2\)`;
- display math as `\[E = mc^2\]`.

For compatibility, the parser also accepts `$x^2$` and `$$E = mc^2$$` under
conservative rules.

Parsing precedence:

1. Fenced code and inline code.
2. Escaped delimiters.
3. Display math.
4. Inline math.
5. Existing Markdown spans and citations.

Unmatched delimiters remain literal. Dollar parsing must distinguish ordinary
currency such as `$10 and $20`. Display expressions preserve internal newlines.

### 3. Render with local KaTeX

`MathExpression.svelte` calls the KaTeX DOM API for an already parsed math node.
Do not use the auto-render extension to rescan the whole answer after every
token.

KaTeX and its fonts ship in the application bundle. Rendering performs no CDN
or runtime network request.

Use:

- `trust: false`;
- finite expansion and size limits;
- accessible MathML output;
- horizontal scrolling for long display equations.

Render with exceptions enabled, catch failures, and show the original `raw`
source as ordinary Svelte text. Do not expose raw KaTeX exception messages in
the UI.

“Always well formatted” means valid supported TeX always typesets, while bad
input always remains readable and never damages the surrounding answer.

### 4. Make streaming stable

The existing answer is reparsed as deltas arrive.

- A closed expression renders immediately.
- An incomplete expression remains literal until its closing delimiter arrives.
- Completed formula components are keyed by expression and display mode.
- The persisted final answer uses exactly the same parser and renderer.

Partial TeX must never be sent to KaTeX merely because a stream paused between
tokens.

### 5. Use the same behavior in notes

Saved notes use the block parser rather than an inline-only subset, allowing
display equations. Citations and `[@vault/paper]` references continue to resolve
through their existing rendering paths.

## Scope

### In scope

- Inline and display math in streamed and stored chat answers.
- Inline and display math in saved notes.
- KaTeX packaging, fonts, safety limits, fallback, and accessibility.
- Parser, streaming-prefix, component, and packaged-app tests.

### Out of scope

- Full LaTeX documents or package loading.
- Equation numbering, user-defined macros, or a visual equation editor.
- OCR or extracting equations from PDFs.
- Replacing the Markdown parser with Remark/Rehype.
- Changing imported HTML MathML behavior.

## Relevant Code

- `src/lib/features/reader/markdown.ts`
- `src/lib/features/reader/markdown.test.ts`
- `src/lib/features/reader/CitedAnswer.svelte`
- `src/lib/features/reader/NoteText.svelte`
- `src/lib/features/reader/ReaderInspector.svelte`
- `src-tauri/src/services/chat/context.rs`

## Risks

- Single-dollar delimiters conflict with currency.
- KaTeX does not support every LaTeX command.
- Large expressions can consume rendering time without limits.
- Block rendering in notes may alter whitespace if not regression-tested.
- Fonts may work in development but be absent from a packaged Tauri build.

## Acceptance Criteria

- [x] Inline and display equations render in completed and streaming answers.
- [x] The same syntax renders in saved notes.
- [x] Code spans and fences never interpret delimiters as math.
- [x] Currency, escaped dollars, unmatched delimiters, and invalid TeX remain
  readable.
- [x] Citations and paper references behave exactly as before.
- [x] Math cannot introduce scripts, external loads, custom HTML, or arbitrary
  attributes.
- [x] Long equations do not widen or overflow the inspector.
- [x] Tests cover every supported delimiter, multiline display math, currency,
  malformed TeX, and every streamed prefix of representative formulas.
- [ ] A packaged application renders formulas and bundled fonts offline.

## Implementation Notes

Implemented on 2026-08-24 with typed inline and display math nodes, a shared
KaTeX renderer for chat and notes, literal fallback on rendering errors, and a
canonical math-output instruction in the chat prompt. The production frontend
build emits KaTeX and its fonts locally; visual verification in a packaged
Tauri application remains outstanding.

Verification:

- `node --test src/lib/features/reader/markdown.test.ts`
- `cargo test services::chat::context::tests::context_requests_canonical_math_delimiters`
- `pnpm check`
- `pnpm build`
