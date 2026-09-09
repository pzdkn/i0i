# Research Loop Evaluation

This explicit acceptance runner belongs to RFC 0126. It is excluded from normal
tests and default CI because the completed scenarios use a real Codex runtime,
model calls, and live services.

Use unit and deterministic integration tests for routine refactors. Reserve this
paid live evaluation for critical research-loop changes or an explicit request;
agree on a bounded set of scenarios rather than retrying after each edit.

Run the offline contract tests:

```sh
python3 -m unittest scripts/research_eval/test_run.py
```

RFC 0147 also exercises the production controller with a deterministic fake
Codex process, real HTTP MCP calls, and isolated SQLite. From `src-tauri`:

```sh
cargo test --no-default-features --lib synthesis
```

This covers one successful correction, a rejected correction, cancellation,
deadlines, transactional failures, and recovery without paid model calls.

The model-backed scenarios require:

- `codex` on `PATH`, authenticated and able to use both selected models;
- `src-tauri/resources/pdfium/libpdfium.dylib` for the fixed-corpus PDFs;
- `src-tauri/resources/obscura/obscura` and configured discovery credentials for
  the live-discovery scenario.

From the repository root, first run one controlled scenario:

```sh
pnpm eval:research --scenario one_iteration \
  --agent-model gpt-5.6-sol \
  --judge-model gpt-5.5 \
  --run-timeout 900 \
  --scenario-timeout 1200
```

This case starts with empty Research State, requires both cached HTML and PDF
evidence, and checks honest abstract-only/unavailable coverage. Then verify
continuation across two fresh agent Runs:

```sh
pnpm eval:research --scenario multiple_iterations \
  --agent-model gpt-5.6-sol \
  --judge-model gpt-5.5 \
  --run-timeout 900 \
  --scenario-timeout 2100
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

Reports include bounded synthesis proposals, validation failures, attempt/turn
ids, and commit receipts. They inspect the actual runtime tool catalog rather
than assuming configuration equals visibility. The independent judge runs only
after deterministic integrity checks pass. A timed-out or invalid run remains
a failed acceptance result even when some tools completed successfully.
