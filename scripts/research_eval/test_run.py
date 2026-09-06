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

    def test_missing_production_adapter_reports_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            checks = RUNNER.evaluate_scenario("one_iteration", Path(directory))
        self.assertEqual(checks[0].status, "blocked")
        self.assertIn("RFCs 0127-0135", checks[0].detail)

    def test_markdown_contains_auditable_result(self) -> None:
        report = {
            "status": "blocked",
            "scenario": "one_iteration",
            "git": {"revision": "abc", "dirty": False},
            "started_at": "2026-09-06T00:00:00+00:00",
            "checks": [{"name": "adapter", "status": "blocked", "detail": "missing"}],
            "judge": {"detail": "Judge did not run."},
        }
        markdown = RUNNER.render_markdown(report)
        self.assertIn("Status: blocked", markdown)
        self.assertIn("`adapter`", markdown)
        self.assertIn("Judge did not run", markdown)


if __name__ == "__main__":
    unittest.main()
