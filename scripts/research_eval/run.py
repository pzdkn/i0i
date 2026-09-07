#!/usr/bin/env python3
"""Run the explicit Milestone 00 research-loop acceptance evaluation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
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


def codex_version() -> str:
    """Return the installed Codex version used by the acceptance run."""

    try:
        completed = subprocess.run(
            ["codex", "--version"], check=True, capture_output=True, text=True
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return "unavailable"
    return completed.stdout.strip() or "unreported"


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
    """Check whether the production-backed Rust scenario adapter is present."""

    marker = (
        repo
        / "src-tauri"
        / "src"
        / "services"
        / "research"
        / "research_eval.rs"
    )
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
    if shutil.which("codex") is None:
        return [
            CheckResult(
                name="codex_runtime",
                status="blocked",
                detail="The codex executable is not available on PATH.",
            )
        ]
    return [
        CheckResult(
            name="production_research_eval_boundary",
            status="pass",
            detail=f"Rust adapter is available for {scenario!r}.",
        )
    ]


def run_backend_scenario(
    scenario: str,
    repo: Path,
    agent_model: str,
    judge_model: str,
    raw_output: Path,
    timeout_seconds: int,
    run_timeout_seconds: int,
) -> dict[str, Any]:
    """Run one ignored Rust acceptance scenario through the production controller."""

    preflight = evaluate_scenario(scenario, repo)
    if overall_status(preflight) != "pass":
        return {
            "schemaVersion": 1,
            "scenario": scenario,
            "status": overall_status(preflight),
            "checks": [asdict(check) for check in preflight],
            "judge": None,
            "errors": [],
        }
    env = os.environ.copy()
    env.update(
        {
            "I0I_RESEARCH_EVAL_SCENARIO": scenario,
            "I0I_RESEARCH_EVAL_OUTPUT": str(raw_output.resolve()),
            "I0I_RESEARCH_EVAL_AGENT_MODEL": agent_model,
            "I0I_RESEARCH_EVAL_JUDGE_MODEL": judge_model,
            "I0I_RESEARCH_EVAL_RUN_SECONDS": str(run_timeout_seconds),
        }
    )
    command = [
        "cargo",
        "test",
        "--no-default-features",
        "services::research::research_eval::tests::explicit_scenario",
        "--lib",
        "--",
        "--ignored",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]
    try:
        completed = subprocess.run(
            command,
            cwd=repo / "src-tauri",
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        return {
            "schemaVersion": 1,
            "scenario": scenario,
            "status": "blocked",
            "checks": [],
            "judge": None,
            "errors": [f"Scenario process exceeded {timeout_seconds}s: {error}"],
        }
    if not raw_output.exists():
        diagnostics = "\n".join((completed.stdout, completed.stderr)).strip()
        return {
            "schemaVersion": 1,
            "scenario": scenario,
            "status": "fail",
            "checks": [],
            "judge": None,
            "errors": [
                f"Rust scenario exited {completed.returncode} without a report: "
                f"{diagnostics[-4000:]}"
            ],
        }
    report = json.loads(raw_output.read_text(encoding="utf-8"))
    report["process"] = {
        "exitCode": completed.returncode,
        "stdoutTail": completed.stdout[-4000:],
        "stderrTail": completed.stderr[-4000:],
    }
    if completed.returncode != 0:
        report["status"] = "fail"
        report.setdefault("errors", []).append(
            f"Rust scenario process exited with status {completed.returncode}."
        )
    return report


def overall_status(checks: list[CheckResult]) -> str:
    """Return pass, fail, or blocked from deterministic checks."""

    if any(check.status == "fail" for check in checks):
        return "fail"
    if any(check.status == "blocked" for check in checks):
        return "blocked"
    return "pass"


def aggregate_status(statuses: list[str]) -> str:
    """Combine scenario statuses without hiding blocked prerequisites."""

    if any(status == "fail" for status in statuses):
        return "fail"
    if any(status == "blocked" for status in statuses):
        return "blocked"
    return "pass" if statuses else "fail"


def render_markdown(report: dict[str, Any]) -> str:
    """Render a concise human-reviewable report from the JSON result."""

    lines = [
        "# Research Loop Evaluation",
        "",
        f"- Status: {report['status']}",
        f"- Scenario: {report['scenario']}",
        f"- Revision: `{report['git']['revision']}`",
        f"- Dirty worktree: {str(report['git']['dirty']).lower()}",
        f"- Codex: `{report['runtime']['codexVersion']}`",
        f"- Agent model: `{report['models']['agent']}`",
        f"- Judge model: `{report['models']['judge']}`",
        f"- Scenario process limit: {report['executionLimits']['scenarioProcessSeconds']}s",
        f"- Research Run limit: {report['executionLimits']['maximumRunSeconds']}s",
        f"- Started: {report.get('startedAt', report.get('started_at', 'unreported'))}",
        "",
        "## Checks",
        "",
    ]
    for check in report["checks"]:
        lines.append(f"- **{check['status']}** `{check['name']}`: {check['detail']}")
    lines.extend(["", "## LLM Judge", ""])
    judge = report.get("judge")
    if not judge:
        lines.append("Judge did not produce a conclusive result.")
    else:
        lines.append(judge.get("summary", "No judge summary."))
        lines.append("")
        for name, result in judge.get("dimensions", {}).items():
            lines.append(
                f"- `{name}`: {result.get('score')} - {result.get('explanation', '')}"
            )
    errors = report.get("errors", [])
    if errors:
        lines.extend(["", "## Errors", ""])
        lines.extend(f"- {error}" for error in errors)
    lines.append("")
    return "\n".join(lines)


def render_suite_markdown(report: dict[str, Any]) -> str:
    """Render the aggregate result of one complete acceptance invocation."""

    lines = [
        "# Research Loop Acceptance Suite",
        "",
        f"- Status: {report['status']}",
        f"- Revision: `{report['git']['revision']}`",
        f"- Dirty worktree: {str(report['git']['dirty']).lower()}",
        f"- Codex: `{report['runtime']['codexVersion']}`",
        f"- Agent model: `{report['models']['agent']}`",
        f"- Judge model: `{report['models']['judge']}`",
        "",
        "## Scenarios",
        "",
    ]
    for scenario in report["scenarios"]:
        lines.append(
            f"- **{scenario['status']}** `{scenario['name']}`: "
            f"[{scenario['report']}]({scenario['report']})"
        )
    lines.append("")
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse the small, explicit evaluation command surface."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=(*SCENARIOS, "all"), default="all")
    parser.add_argument("--agent-model", required=True)
    parser.add_argument("--judge-model", required=True)
    parser.add_argument("--scenario-timeout", type=int, default=900)
    parser.add_argument("--run-timeout", type=int, default=420)
    parser.add_argument("--output-dir", type=Path, default=Path("artifacts/research-eval"))
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run selected scenarios and retain JSON and Markdown reports."""

    args = parse_args(argv or sys.argv[1:])
    script_dir = Path(__file__).resolve().parent
    repo = script_dir.parents[1]
    manifest = load_manifest(script_dir)
    if args.agent_model == args.judge_model:
        raise SystemExit("--judge-model must differ from --agent-model")
    if args.scenario_timeout <= 0 or args.run_timeout <= 0:
        raise SystemExit("--scenario-timeout and --run-timeout must be positive")
    scenarios = SCENARIOS if args.scenario == "all" else (args.scenario,)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    started_at = datetime.now(timezone.utc)
    runtime = {"codexVersion": codex_version()}
    git = git_metadata(repo)
    statuses: list[str] = []
    scenario_reports: list[dict[str, str]] = []
    for scenario in scenarios:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        stem = args.output_dir / f"{stamp}-{scenario}"
        raw_output = stem.with_suffix(".raw.json")
        report = run_backend_scenario(
            scenario,
            repo,
            args.agent_model,
            args.judge_model,
            raw_output,
            args.scenario_timeout,
            args.run_timeout,
        )
        report["git"] = git
        report["runtime"] = runtime
        report["models"] = {"agent": args.agent_model, "judge": args.judge_model}
        report.setdefault("executionLimits", {})["scenarioProcessSeconds"] = (
            args.scenario_timeout
        )
        report["manifest"] = {
            "version": manifest["version"],
            "sha256": sha256_file(script_dir / "corpus" / "manifest.json"),
        }
        status = report["status"]
        statuses.append(status)
        stem.with_suffix(".json").write_text(
            json.dumps(report, indent=2) + "\n", encoding="utf-8"
        )
        stem.with_suffix(".md").write_text(render_markdown(report), encoding="utf-8")
        scenario_reports.append(
            {
                "name": scenario,
                "status": status,
                "report": stem.with_suffix(".md").name,
            }
        )
        print(f"{scenario}: {status} ({stem.with_suffix('.md')})")

    if args.scenario == "all":
        suite_status = aggregate_status(statuses)
        suite = {
            "schemaVersion": 1,
            "status": suite_status,
            "startedAt": started_at.isoformat(),
            "git": git,
            "runtime": runtime,
            "models": {"agent": args.agent_model, "judge": args.judge_model},
            "scenarios": scenario_reports,
        }
        suite_stem = args.output_dir / (
            started_at.strftime("%Y%m%dT%H%M%SZ") + "-suite"
        )
        suite_stem.with_suffix(".json").write_text(
            json.dumps(suite, indent=2) + "\n", encoding="utf-8"
        )
        suite_stem.with_suffix(".md").write_text(
            render_suite_markdown(suite), encoding="utf-8"
        )

    return 0 if statuses and all(status == "pass" for status in statuses) else 2


if __name__ == "__main__":
    raise SystemExit(main())
