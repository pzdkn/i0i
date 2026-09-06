# Research Loop Evaluation

This explicit acceptance runner belongs to RFC 0126. It is excluded from normal
tests and default CI because the completed scenarios use a real Codex runtime,
model calls, and live services.

Run the offline contract tests:

```sh
python3 -m unittest scripts/research_eval/test_run.py
```

Run the acceptance suite on demand:

```sh
pnpm eval:research --scenario all \
  --agent-model MODEL_ID \
  --judge-model DIFFERENT_MODEL_ID
```

Until RFCs 0127 through 0135 provide the production boundary, this command exits
with status 2 and writes honest `blocked` reports under `artifacts/research-eval`.
The runner never reads or writes the user's application database.
