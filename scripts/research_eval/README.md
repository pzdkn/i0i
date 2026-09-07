# Research Loop Evaluation

This explicit acceptance runner belongs to RFC 0126. It is excluded from normal
tests and default CI because the completed scenarios use a real Codex runtime,
model calls, and live services.

Run the offline contract tests:

```sh
python3 -m unittest scripts/research_eval/test_run.py
```

The model-backed scenarios require:

- `codex` on `PATH`, authenticated and able to use both selected models;
- `src-tauri/resources/pdfium/libpdfium.dylib` for the fixed-corpus PDFs;
- `src-tauri/resources/obscura/obscura` and configured discovery credentials for
  the live-discovery scenario.

From the repository root, first run one controlled scenario:

```sh
pnpm eval:research --scenario one_iteration \
  --agent-model gpt-5.6-sol \
  --judge-model gpt-5.5
```

Then run the complete acceptance suite:

```sh
pnpm eval:research --scenario all \
  --agent-model gpt-5.6-sol \
  --judge-model gpt-5.5 \
  --run-timeout 420 \
  --scenario-timeout 900
```

The agent and judge models must differ. `--run-timeout` bounds each production
Research Run; `--scenario-timeout` bounds the complete Rust scenario, including
setup and judging. Increase the latter for a slow live acquisition without
removing the per-Run bound.

Each attempt writes a machine-readable JSON report, a concise Markdown report,
and the raw Rust report under `artifacts/research-eval`. A complete pass exits
with status 0. Failed prerequisites are reported as blocked and all other failed
acceptance checks exit with status 2. The runner uses an isolated temporary
database and document cache, verifies reopening and historical State access, and
removes that temporary data before a scenario can pass. It never reads or writes
the user's application database.
