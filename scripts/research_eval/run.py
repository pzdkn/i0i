#!/usr/bin/env python3
"""Run the explicit Milestone 00 research-loop acceptance evaluation."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCENARIOS: tuple[str, ...] = (
    "one_iteration",
    "multiple_iterations",
    "new_run_continuation",
    "live_discovery_smoke",
)


@dataclass(frozen=True)
class CheckResult:
    """One deterministic prerequisite or scenario result."""

    name: str
    status: str
    detail: str


def sha256_file(path: Path) -> str:
    """Return the hexadecimal SHA-256 digest of a file."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def git_metadata(repo: Path) -> dict[str, Any]:
    """Read auditable revision metadata without modifying the repository."""

    def run(*args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=repo, check=True, capture_output=True, text=True
        ).stdout.strip()

    return {
        "revision": run("rev-parse", "HEAD"),
        "dirty": bool(run("status", "--porcelain")),
    }


def load_manifest(script_dir: Path) -> dict[str, Any]:
    """Load and validate the versioned scenario manifest."""

    manifest_path = script_dir / "corpus" / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("version") != 1:
        raise ValueError("Unsupported research evaluation manifest version")
    configured = tuple(item["id"] for item in manifest.get("scenarios", []))
    if configured != SCENARIOS:
        raise ValueError("Manifest scenarios do not match the runner contract")
    return manifest


def evaluate_scenario(scenario: str, repo: Path) -> list[CheckResult]:
    """Evaluate an available production scenario or report its exact blocker."""

    marker = repo / "src-tauri" / "src" / "services" / "research_eval.rs"
    if not marker.exists():
        return [
            CheckResult(
                name="production_research_eval_boundary",
                status="blocked",
                detail=(
                    "Production evaluation adapter is unavailable. Implement RFCs "
                    "0127-0135 before this scenario can execute."
                ),
            )
        ]
    return [
        CheckResult(
            name="production_research_eval_boundary",
            status="blocked",
            detail=(
                f"The adapter marker exists, but scenario {scenario!r} is not yet "
                "wired to this runner."
            ),
        )
    ]


def overall_status(checks: list[CheckResult]) -> str:
    """Return pass, fail, or blocked from deterministic checks."""

    if any(check.status == "fail" for check in checks):
        return "fail"
    if any(check.status == "blocked" for check in checks):
        return "blocked"
    return "pass"


def render_markdown(report: dict[str, Any]) -> str:
    """Render a concise human-reviewable report from the JSON result."""

    lines = [
        "# Research Loop Evaluation",
        "",
        f"- Status: {report['status']}",
        f"- Scenario: {report['scenario']}",
        f"- Revision: `{report['git']['revision']}`",
        f"- Dirty worktree: {str(report['git']['dirty']).lower()}",
        f"- Started: {report['started_at']}",
        "",
        "## Checks",
        "",
    ]
    for check in report["checks"]:
        lines.append(f"- **{check['status']}** `{check['name']}`: {check['detail']}")
    lines.extend(
        [
            "",
            "## LLM Judge",
            "",
            report["judge"]["detail"],
            "",
        ]
    )
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse the small, explicit evaluation command surface."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=(*SCENARIOS, "all"), default="all")
    parser.add_argument("--agent-model")
    parser.add_argument("--judge-model")
    parser.add_argument("--output-dir", type=Path, default=Path("artifacts/research-eval"))
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run selected scenarios and retain JSON and Markdown reports."""

    args = parse_args(argv or sys.argv[1:])
    script_dir = Path(__file__).resolve().parent
    repo = script_dir.parents[1]
    manifest = load_manifest(script_dir)
    scenarios = SCENARIOS if args.scenario == "all" else (args.scenario,)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    statuses: list[str] = []
    for scenario in scenarios:
        checks = evaluate_scenario(scenario, repo)
        status = overall_status(checks)
        statuses.append(status)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        report = {
            "schema_version": 1,
            "status": status,
            "scenario": scenario,
            "started_at": datetime.now(timezone.utc).isoformat(),
            "git": git_metadata(repo),
            "models": {"agent": args.agent_model, "judge": args.judge_model},
            "manifest": {
                "version": manifest["version"],
                "sha256": sha256_file(script_dir / "corpus" / "manifest.json"),
            },
            "checks": [asdict(check) for check in checks],
            "judge": {
                "status": "blocked",
                "detail": "Judge runs only after deterministic production checks pass.",
            },
        }
        stem = args.output_dir / f"{stamp}-{scenario}"
        stem.with_suffix(".json").write_text(
            json.dumps(report, indent=2) + "\n", encoding="utf-8"
        )
        stem.with_suffix(".md").write_text(render_markdown(report), encoding="utf-8")
        print(f"{scenario}: {status} ({stem.with_suffix('.md')})")

    return 0 if statuses and all(status == "pass" for status in statuses) else 2


if __name__ == "__main__":
    raise SystemExit(main())
