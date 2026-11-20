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
bump_spec = importlib.util.spec_from_file_location("bump_version", ROOT / "scripts/bump_version.py")
bump = importlib.util.module_from_spec(bump_spec)
bump_spec.loader.exec_module(bump)


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
        self.assertIn('DeleteRegKey HKCU "${MANUPRODUCTKEY}"', text)  # 2.0.1：解除時清本產品鍵
        self.assertNotRegex(text, r'DeleteRegKey \w+ "\$\{MANUKEY\}"')  # 不刪製造商上層

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

    def test_ci_release_build_keeps_webview2_bootstrapper_and_only_skips_build_time_signing(self):
        override = json.loads((ROOT / "app/src-tauri/tauri.ci-release.conf.json").read_text(encoding="utf-8"))
        release.validate_ci_release_config(override)
        self.assertNotIn("webviewInstallMode", json.dumps(override))
        main = json.loads((ROOT / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        self.assertNotIn("webviewInstallMode", main["bundle"].get("windows", {}))  # 預設＝自動補裝WebView2
        with self.assertRaises(ValueError):
            release.validate_ci_release_config({"bundle": {"createUpdaterArtifacts": False, "windows": {"webviewInstallMode": {"type": "skip"}}}})
        workflow = json.loads((ROOT / ".github/workflows/package-windows.yml").read_text(encoding="utf-8"))
        runs = [s.get("run", "") for s in workflow["jobs"]["package"]["steps"]]
        self.assertTrue(any("build-windows-test" in r and "--profile ci-release" in r for r in runs))

    def test_ci_release_profile_passes_its_own_config_to_tauri(self):
        with patch.object(release, "nsis_preflight", return_value=[]), patch.object(release, "prepare"), \
                patch.object(release.shutil, "which", return_value="npm"), patch.object(release.subprocess, "run") as run, \
                patch.object(release.sys, "platform", "win32"), patch.object(release.Path, "glob", return_value=[]):
            with self.assertRaisesRegex(ValueError, "exactly one"):
                release.build_windows_test(False, "ci-release")
        args = run.call_args[0][0]
        self.assertEqual(args[args.index("--config") + 1], "src-tauri/tauri.ci-release.conf.json")
        self.assertIn("--no-sign", args)
        with self.assertRaisesRegex(ValueError, "unknown"):
            release.build_windows_test(False, "release")

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

    def test_sbom_serial_number_is_valid_stable_and_content_bound(self):
        # actions/attest只接受同時有bomFormat、specVersion、serialNumber的CycloneDX
        bom = json.loads((ROOT / "target/release-support/SBOM.cdx.json").read_text(encoding="utf-8"))
        for key in ("bomFormat", "specVersion", "serialNumber"):
            self.assertIn(key, bom)
        self.assertRegex(bom["serialNumber"], r"^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        self.assertEqual(release.with_serial_number(bom), bom)
        changed = dict(bom, version=2)
        self.assertNotEqual(release.with_serial_number(changed)["serialNumber"], bom["serialNumber"])

    def _fixture(self, folder):
        """在 TEMP 放一份最小版本檔（取自真 repo），不動 repo 本身。"""
        names = [name for name, _, _ in bump.TARGETS] + ["CHANGELOG.md", "README.md", "docs/使用說明.txt"]
        for name in names:
            (folder / name).parent.mkdir(parents=True, exist_ok=True)
            (folder / name).write_bytes((ROOT / name).read_bytes())
        (folder / "app/src").mkdir(parents=True, exist_ok=True)
        (folder / "app/src/ok.tsx").write_text("const v=await getVersion();", encoding="utf-8")

    def test_bump_version_changes_every_file_together_and_fills_changelog_date(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            self._fixture(folder)
            changelog = folder / "CHANGELOG.md"
            text = changelog.read_bytes().decode("utf-8").replace("## 2.0.0", "## 9.9.9（預定日）\r\n\r\n- ok\r\n\r\n## 2.0.0", 1)
            changelog.write_bytes(text.encode("utf-8"))
            self.assertIn("CHANGELOG 標題要寫成「## 9.9.9（YYYY-MM-DD）」", release.release_doc_problems(folder, "9.9.9"))
            changed = bump.bump(folder, "9.9.9", "2030-01-02")
            self.assertEqual(len(changed), 7)
            self.assertEqual(json.loads((folder / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"], "9.9.9")
            self.assertEqual((folder / "Cargo.lock").read_text(encoding="utf-8").count('version = "9.9.9"'), 2)
            self.assertIn("\r\n## 9.9.9（2030-01-02）\r\n", changelog.read_bytes().decode("utf-8"))
            self.assertEqual(release.release_doc_problems(folder, "9.9.9"), [])
            before = (folder / "core/Cargo.toml").read_bytes()
            with self.assertRaises(ValueError):
                bump.bump(folder, "9.9.10", "2030-01-02")  # CHANGELOG 沒有這版 → 整批不寫
            self.assertEqual((folder / "core/Cargo.toml").read_bytes(), before)

    def test_release_docs_block_hardcoded_version_and_unreleased_wording(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            self._fixture(folder)
            self.assertEqual(release.release_doc_problems(folder, "2.0.0"), [])
            (folder / "app/src/ok.tsx").write_text("const big='12.0.01';", encoding="utf-8")
            self.assertEqual(release.release_doc_problems(folder, "2.0.0"), [], "相似數字不誤判")
            (folder / "app/src/bad.tsx").write_text("<b>v2.0.0</b>", encoding="utf-8")
            with (folder / "README.md").open("a", encoding="utf-8") as f:
                f.write("\n2.0.0 尚未正式發佈\n")
            problems = release.release_doc_problems(folder, "2.0.0")
            self.assertTrue(any("app/src/bad.tsx" in p for p in problems))
            self.assertTrue(any("README.md" in p and "尚未正式" in p for p in problems))


if __name__ == "__main__":
    unittest.main()
