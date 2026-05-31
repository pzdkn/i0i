# Extractor Spike

This folder is the RFC 0025 evaluation spike. It compares PDF extraction tools before i0i commits to a production `DocumentExtraction` adapter.

The spike is intentionally separate from the Tauri app runtime. It can use heavyweight or experimental tools without adding them to the shipped app.

## Outputs

The spike should produce:

- fetched corpus PDFs under `corpus/pdfs/`
- raw extractor output under `outputs/{extractor}/raw/`
- normalized i0i-shaped JSON under `normalized/{extractor}/`
- an HTML preview under `previews/index.html`
- a Markdown decision report under `report.md`

The decision report should end with one of:

- winner chosen
- tie / no winner
- second-pass evaluation required

The current short conclusion lives in `SPIKE_REPORT.md`.

## Corpus

The corpus is fixed and manifest-driven:

```sh
pnpm spike:extractors:fetch
```

PDFs are intentionally ignored by git. The manifest is the reproducible source of truth.

## Run

Install extractor CLIs into isolated `uv` virtual environments:

```sh
bash scripts/extractor_spike/setup_tools.sh all
```

You can also install one tool at a time:

```sh
bash scripts/extractor_spike/setup_tools.sh docling
bash scripts/extractor_spike/setup_tools.sh marker
bash scripts/extractor_spike/setup_tools.sh mineru
```

The runner defaults to these local venv commands:

- `scripts/extractor_spike/.venvs/docling/bin/docling`
- `scripts/extractor_spike/.venvs/marker/bin/marker_single`
- `scripts/extractor_spike/.venvs/mineru/bin/mineru`

```sh
pnpm spike:extractors
```

For a quick smoke test, run one paper through one extractor:

```sh
pnpm spike:extractors --paper attention-is-all-you-need --tool docling
```

For slower extractors, raise the per-paper timeout:

```sh
pnpm spike:extractors --timeout-seconds 1800
```

You can override commands with environment variables:

```sh
I0I_DOCLING_CMD="docling {pdf} --to json --image-export-mode referenced --device cpu --output {raw_dir}" \
I0I_MARKER_CMD="marker_single {pdf} --output_dir {raw_dir} --output_format json" \
I0I_MINERU_CMD="mineru -p {pdf} -o {raw_dir} -b pipeline" \
I0I_PDFIUM_BASIC_CMD="your-pdfium-baseline-command" \
python3 scripts/extractor_spike/run_spike.py
```

Command templates may use:

- `{pdf}` for the input PDF path
- `{raw_dir}` for the extractor raw output directory
- `{paper_id}` for the corpus paper id

Example:

```sh
I0I_DOCLING_CMD="docling {pdf} --to json --output {raw_dir}"
```

## Scorecard

Scores use the RFC scale:

```text
0 = unusable
1 = poor
2 = acceptable with manual cleanup
3 = good enough for first production adapter
4 = strong
5 = excellent
```

The generated `report.md` starts as a structured worksheet. Human judgement still matters here; the spike is meant to make that judgement evidence-backed.
