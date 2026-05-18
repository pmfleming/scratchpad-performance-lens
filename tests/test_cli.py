from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRATCHPAD = ROOT.parent / "scratchpad"


class CliTests(unittest.TestCase):
    def run_lens(self, *args: str) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["PYTHONPATH"] = str(ROOT / "src")
        return subprocess.run(
            [
                sys.executable,
                "-m",
                "scratchpad_performance_lens.cli",
                *args,
                "--config",
                str(ROOT / "examples" / "scratchpad.toml"),
            ],
            cwd=ROOT,
            env=env,
            text=True,
            capture_output=True,
        )

    def test_catalog_lists_performance_and_telemetry_tasks(self) -> None:
        result = self.run_lens("catalog")

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        task_ids = {task["id"] for task in payload["tasks"]}
        self.assertIn("performance.search", task_ids)
        self.assertIn("performance.performance-review", task_ids)
        self.assertIn("telemetry.app-package", task_ids)

    def test_project_code_metrics_runs_against_scratchpad(self) -> None:
        if not SCRATCHPAD.exists():
            self.skipTest("Scratchpad checkout is not available beside this repo")

        result = self.run_lens("measure", "project-code")

        self.assertEqual(result.returncode, 0, result.stderr)
        output = SCRATCHPAD / "target" / "analysis" / "project_code_metrics.json"
        self.assertTrue(output.exists())
        payload = json.loads(output.read_text(encoding="utf-8"))
        self.assertIn("current", payload)
        self.assertIn("history", payload)

    def test_telemetry_prints_json(self) -> None:
        if not SCRATCHPAD.exists():
            self.skipTest("Scratchpad checkout is not available beside this repo")

        result = self.run_lens("telemetry")

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertIn("manifest", payload)


if __name__ == "__main__":
    unittest.main()
