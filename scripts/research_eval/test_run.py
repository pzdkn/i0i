"""Focused offline tests for the research evaluation contract."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("run.py")
SPEC = importlib.util.spec_from_file_location("research_eval_run", MODULE_PATH)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)


class ResearchEvaluationTests(unittest.TestCase):
    """Validate status and reporting without starting models or the app."""

    def test_blocked_check_cannot_pass(self) -> None:
        checks = [RUNNER.CheckResult("adapter", "blocked", "missing")]
        self.assertEqual(RUNNER.overall_status(checks), "blocked")

    def test_failure_takes_precedence_over_blocked(self) -> None:
        checks = [
            RUNNER.CheckResult("adapter", "blocked", "missing"),
            RUNNER.CheckResult("reference", "fail", "invalid"),
        ]
        self.assertEqual(RUNNER.overall_status(checks), "fail")

    def test_suite_status_preserves_blocked_result(self) -> None:
        self.assertEqual(RUNNER.aggregate_status(["pass", "blocked"]), "blocked")
        self.assertEqual(RUNNER.aggregate_status(["blocked", "fail"]), "fail")

    def test_missing_production_adapter_reports_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            checks = RUNNER.evaluate_scenario("one_iteration", Path(directory))
        self.assertEqual(checks[0].status, "blocked")
        self.assertIn("RFCs 0127-0135", checks[0].detail)

    def test_nested_production_adapter_is_discovered(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            marker = repo / "src-tauri/src/services/research/research_eval.rs"
            marker.parent.mkdir(parents=True)
            marker.write_text("// adapter", encoding="utf-8")
            checks = RUNNER.evaluate_scenario("one_iteration", repo)
        self.assertEqual(checks[0].status, "pass")

    def test_markdown_contains_auditable_result(self) -> None:
        report = {
            "status": "blocked",
            "scenario": "one_iteration",
            "git": {"revision": "abc", "dirty": False},
            "runtime": {"codexVersion": "codex-cli 1.2.3"},
            "models": {"agent": "agent", "judge": "judge"},
            "executionLimits": {
                "scenarioProcessSeconds": 900,
                "maximumRunSeconds": 420,
            },
            "started_at": "2026-09-06T00:00:00+00:00",
            "checks": [{"name": "adapter", "status": "blocked", "detail": "missing"}],
            "judge": None,
        }
        markdown = RUNNER.render_markdown(report)
        self.assertIn("Status: blocked", markdown)
        self.assertIn("`adapter`", markdown)
        self.assertIn("Research Run limit: 420s", markdown)
        self.assertIn("codex-cli 1.2.3", markdown)
        self.assertIn("Judge did not produce", markdown)

    def test_failed_backend_report_receives_configured_execution_limits(self) -> None:
        """The CLI adds both limits even when Rust returns an early failure."""

        report = {"executionLimits": {}}
        RUNNER.add_execution_limits(report, scenario_seconds=900, run_seconds=420)

        self.assertEqual(report["executionLimits"]["scenarioProcessSeconds"], 900)
        self.assertEqual(report["executionLimits"]["maximumRunSeconds"], 420)

    def test_suite_markdown_lists_each_scenario(self) -> None:
        report = {
            "status": "pass",
            "git": {"revision": "abc", "dirty": False},
            "runtime": {"codexVersion": "codex-cli 1.2.3"},
            "models": {"agent": "agent", "judge": "judge"},
            "scenarios": [
                {
                    "name": "one_iteration",
                    "status": "pass",
                    "report": "one.md",
                }
            ],
        }
        markdown = RUNNER.render_suite_markdown(report)
        self.assertIn("Status: pass", markdown)
        self.assertIn("`one_iteration`", markdown)
        self.assertIn("(one.md)", markdown)


if __name__ == "__main__":
    unittest.main()
