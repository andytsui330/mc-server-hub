"""Run the configured Windows steps; failing tools must fail the step."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
POWERSHELL = shutil.which("pwsh")


@unittest.skipUnless(os.name == "nt" and POWERSHELL, "Windows PowerShell workflow tests")
class WindowsWorkflowTests(unittest.TestCase):
    def steps(self, name):
        workflow = json.loads((ROOT / ".github/workflows" / name).read_text(encoding="utf-8"))
        return next(iter(workflow["jobs"].values()))["steps"]

    def run_step(self, script, folder):
        tools = folder / "tools"
        tools.mkdir()
        (tools / "cargo.cmd").write_text(
            '@echo off\nif "%1"=="test" exit /b 101\nexit /b 0\n', encoding="ascii"
        )
        (tools / "npm.cmd").write_text("@echo off\nexit /b 101\n", encoding="ascii")
        native_temp = folder / "native-temp"
        native_temp.mkdir()
        runner_temp = folder / "runner-temp"
        runner_temp.mkdir()
        github_env = folder / "github-env.txt"
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   TEMP=str(native_temp), TMP=str(native_temp),
                   RUNNER_TEMP=str(runner_temp), GITHUB_ENV=str(github_env),
                   GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="1")
        # Same failure policy and final native exit-code check as the Actions pwsh shell.
        wrapped = ("$ErrorActionPreference = 'Stop'\n" + script
                   + "\nif (Test-Path variable:\\LASTEXITCODE) { exit $LASTEXITCODE }\n")
        result = subprocess.run([POWERSHELL, "-NoProfile", "-NonInteractive", "-Command", wrapped],
                                env=env, capture_output=True, text=True, timeout=30)
        return result, github_env, native_temp

    def test_rust_failure_fails_the_configured_test_step(self):
        step = next(s for s in self.steps("ci.yml") if "cargo test " in s.get("run", ""))
        with tempfile.TemporaryDirectory(prefix="mchub-ci-exit-") as temp:
            result, _, _ = self.run_step(step["run"], Path(temp))
            self.assertEqual(result.returncode, 101, result.stdout + result.stderr)

    def test_failed_npm_install_fails_each_workflow_step(self):
        for name in ("ci.yml", "package-windows.yml"):
            with self.subTest(workflow=name), tempfile.TemporaryDirectory(prefix="mchub-ci-npm-") as temp:
                step = next(s for s in self.steps(name) if "npm ci " in s.get("run", ""))
                result, _, _ = self.run_step(step["run"], Path(temp))
                self.assertEqual(result.returncode, 101, result.stdout + result.stderr)

    def test_isolated_data_uses_the_native_temp_not_runner_temp(self):
        for name in ("ci.yml", "package-windows.yml"):
            with self.subTest(workflow=name), tempfile.TemporaryDirectory(prefix="mchub-ci-path-") as temp:
                step = next(s for s in self.steps(name) if s.get("name") == "Isolate app data folder")
                result, github_env, native_temp = self.run_step(step["run"], Path(temp))
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                values = dict(line.split("=", 1) for line in github_env.read_text(encoding="utf-8-sig").splitlines())
                data = Path(values["MCHUB_DATA_DIR"])
                self.assertTrue(data.resolve().is_relative_to(native_temp.resolve()), str(data))
                self.assertTrue(data.is_dir())


if __name__ == "__main__":
    unittest.main()
