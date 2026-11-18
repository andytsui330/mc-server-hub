"""第11步：只用新TEMP，測包裝規則；不執行安裝器、不刪檔。"""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release_support", ROOT / "scripts/release_support.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseSupportTests(unittest.TestCase):
    def test_nsis_payload_only_allows_the_one_tauri_bundle_marker_difference(self):
        build = b"MZfixture__TAURI_BUNDLE_TYPE_VAR_UNKsuffix"
        installed = b"MZfixture__TAURI_BUNDLE_TYPE_VAR_NSSsuffix"
        self.assertEqual(release.verify_nsis_payload(installed, build), "UNK->NSS")
        with self.assertRaises(ValueError):
            release.verify_nsis_payload(installed + b"changed", build)
        with self.assertRaises(ValueError):
            release.verify_nsis_payload(installed.replace(b"fixture", b"altered"), build)
        with self.assertRaises(ValueError):
            release.verify_nsis_payload(installed * 2, build * 2)

    def test_changelog_extracts_one_version_and_keeps_other_versions_out(self):
        text = "# 更新紀錄\n\n## 2.0.0（未發佈）\n\n### 升級說明\n- 先備份。\n\n### 新增功能\n- 模組。\n\n## 1.2.0\n舊版內容\n"
        notes = release.release_notes(text, "2.0.0")
        self.assertIn("先備份", notes)
        self.assertNotIn("舊版內容", notes)
        with self.assertRaises(ValueError):
            release.release_notes(text, "9.9.9")

    def test_checksum_uses_fixed_filename_and_detects_changed_bytes(self):
        root = Path(tempfile.mkdtemp(prefix="mchub-release-test-"))
        file = root / "MCServerHub-Setup-Windows.exe"
        file.write_bytes(b"MZfixture")
        before = release.checksum_lines([file])
        self.assertTrue(before.endswith("  MCServerHub-Setup-Windows.exe\n"))
        self.assertNotIn(str(root), before)
        file.write_bytes(b"MZchanged")
        self.assertNotEqual(release.checksum_lines([file]), before)

    def test_missing_tool_is_reported_without_creating_or_downloading_anything(self):
        root = Path(tempfile.mkdtemp(prefix="mchub-no-nsis-"))
        before = list(root.iterdir())
        problems = release.nsis_preflight(root)
        self.assertTrue(problems)
        self.assertEqual(list(root.iterdir()), before)

    def test_build_refuses_missing_tools_before_invoking_any_builder(self):
        root = Path(tempfile.mkdtemp(prefix="mchub-build-gate-"))
        with patch.dict(os.environ, {"LOCALAPPDATA": str(root)}), patch.object(release, "nsis_preflight", return_value=["missing NSIS.exe"]), patch.object(release.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "approval"):
                release.build_windows_test(False)
            run.assert_not_called()

    def test_incomplete_existing_cache_is_preserved_even_when_download_is_allowed(self):
        root = Path(tempfile.mkdtemp(prefix="mchub-partial-cache-"))
        cache = root / "tauri/NSIS"
        cache.mkdir(parents=True)
        file = cache / "makensis.exe"
        file.write_bytes(b"keep this partial fixture")
        with patch.dict(os.environ, {"LOCALAPPDATA": str(root)}), patch.object(release.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "preserved"):
                release.build_windows_test(True)
            run.assert_not_called()
        self.assertEqual(file.read_bytes(), b"keep this partial fixture")

    def test_nsis_template_cannot_create_desktop_shortcuts_or_remove_appdata(self):
        text = (ROOT / "app/src-tauri/windows/installer.nsi").read_text(encoding="utf-8")
        self.assertNotIn('CreateShortcut "$DESKTOP', text)
        self.assertNotIn('RmDir /r "$APPDATA', text)
        self.assertNotIn('RmDir /r "$LOCALAPPDATA', text)
        self.assertNotIn("DeleteAppDataCheckbox", text)
        self.assertIn('CreateShortcut "$SMPROGRAMS', text)

    def test_workflows_are_public_only_read_only_and_actions_are_pinned(self):
        for name in ("ci.yml", "package-windows.yml"):
            workflow = json.loads((ROOT / ".github/workflows" / name).read_text(encoding="utf-8"))
            self.assertEqual(workflow["permissions"], {"contents": "read"})
            for job in workflow["jobs"].values():
                self.assertIn("andytsui330/mc-server-hub", job["if"])
                self.assertIn("!github.event.repository.private", job["if"])
                self.assertNotEqual(job.get("permissions", {}).get("contents"), "write")
                for step in job["steps"]:
                    if "uses" in step:
                        self.assertRegex(step["uses"], r"^actions/[a-z-]+@[a-f0-9]{40}$")
                    self.assertNotIn("gh release", step.get("run", ""))

    def test_installer_config_is_current_user_and_signed_updates_stay_required(self):
        config = json.loads((ROOT / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        release.validate_config(config)
        config["bundle"]["windows"]["nsis"]["installMode"] = "perMachine"
        with self.assertRaises(ValueError):
            release.validate_config(config)

    def test_license_collection_fails_closed_for_missing_text(self):
        root = Path(tempfile.mkdtemp(prefix="mchub-license-test-"))
        with self.assertRaises(ValueError):
            release.license_texts(root, [])
        (root / "LICENSE-MIT").write_text("Copyright fixture\nPermission is hereby granted", encoding="utf-8")
        result = release.license_texts(root, [])
        self.assertIn("Copyright fixture", result)

    def test_sbom_has_unique_refs_resolved_dependencies_and_no_machine_paths(self):
        bom = json.loads((ROOT / "target/release-support/SBOM.cdx.json").read_text(encoding="utf-8"))
        self.assertEqual(bom["bomFormat"], "CycloneDX")
        refs = [bom["metadata"]["component"]["bom-ref"]] + [c["bom-ref"] for c in bom["components"]]
        self.assertEqual(len(refs), len(set(refs)))
        for entry in bom["dependencies"]:
            self.assertIn(entry["ref"], refs)
            self.assertTrue(set(entry["dependsOn"]).issubset(refs))
        raw = json.dumps(bom)
        self.assertNotIn("C:\\\\", raw)
        self.assertNotIn("D:\\\\", raw)
        self.assertGreater(len(bom["components"]), 250)


if __name__ == "__main__":
    unittest.main()
