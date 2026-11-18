"""Explicitly authorized install/uninstall test. No real servers; no recursive filesystem cleanup."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import winreg
import importlib.util

ROOT = Path(__file__).resolve().parents[1]
support_spec = importlib.util.spec_from_file_location("release_support", ROOT / "scripts/release_support.py")
support = importlib.util.module_from_spec(support_spec)
support_spec.loader.exec_module(support)
EVIDENCE = ROOT / "target/step11-install"
PRODUCT = "MC開服平台"
VIEWS = [winreg.KEY_WOW64_64KEY, winreg.KEY_WOW64_32KEY]
PARENT = r"Software\MC Server Hub contributors"
RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"


def sha(file):
    return hashlib.sha256(Path(file).read_bytes()).hexdigest()


def tree(folder):
    return {p.relative_to(folder).as_posix(): sha(p) for p in Path(folder).rglob("*") if p.is_file()}


def registry(hive, path, view):
    try:
        with winreg.OpenKey(hive, path, 0, winreg.KEY_READ | view) as key:
            values, children = {}, {}
            for i in range(winreg.QueryInfoKey(key)[1]):
                name, value, kind = winreg.EnumValue(key, i)
                values[name] = [value, kind]
            for i in range(winreg.QueryInfoKey(key)[0]):
                child = winreg.EnumKey(key, i)
                children[child] = registry(hive, path + "\\" + child, view)
            return {"values": values, "children": children}
    except FileNotFoundError:
        return None


def known_folder(number):
    buf = ctypes.create_unicode_buffer(32768)
    assert ctypes.windll.shell32.SHGetFolderPathW(None, number, None, 0, buf) == 0
    return Path(buf.value)


def run_value(view):
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN, 0, winreg.KEY_READ | view) as key:
        try:
            return list(winreg.QueryValueEx(key, PRODUCT))
        except FileNotFoundError:
            return None


def shortcut_target(file):
    env = {**os.environ, "MCHUB_TEST_SHORTCUT": str(file)}
    command = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new();(New-Object -ComObject WScript.Shell).CreateShortcut($env:MCHUB_TEST_SHORTCUT).TargetPath"
    result = subprocess.run(["powershell.exe", "-NoProfile", "-Command", command], env=env, check=True, capture_output=True)
    return Path(result.stdout.decode("utf-8-sig").strip()).resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--execute", action="store_true", help="Use only after Andy approves this exact candidate and scope")
    parser.add_argument("--resume-installed", action="store_true", help="Only resume an owned installation recorded by this test; never install twice")
    args = parser.parse_args()
    assert args.execute, "No installation without explicit --execute and user approval"
    plan = json.loads(args.plan.read_text(encoding="utf-8"))
    workspace = Path(plan["workspace"]).resolve()
    temp = Path(tempfile.gettempdir()).resolve()
    assert workspace.parent == temp and workspace.name.startswith("mchub-nsis-test-")
    app, data, server = [Path(plan[k]).resolve() for k in ("install_dir", "data_dir", "fake_server_dir")]
    assert app == workspace / "app" and data == workspace / "data" and server == workspace / "fake-server"
    previous = None
    if args.resume_installed:
        previous = json.loads((EVIDENCE / "report.json").read_text(encoding="utf-8"))
        assert Path(previous["workspace"]).resolve() == workspace and previous.get("installer_exit") == 0
        assert previous.get("failure") is not None and previous["phase"] == "preflight passed; about to install"
        (EVIDENCE / "first-attempt.json").write_text(json.dumps(previous, ensure_ascii=False, indent=2), encoding="utf-8")
    else:
        assert not app.exists(), "Do not overwrite an existing install directory"
    assert sha(plan["installer"]) == plan["installer_sha256"]
    for name, expected in plan["fixture_sha256_before"].items():
        file = (workspace / name).resolve()
        assert file.is_relative_to(workspace) and sha(file) == expected
    keys = plan["hkcu_keys_expected_absent"]
    assert keys == [r"Software\Microsoft\Windows\CurrentVersion\Uninstall\MC開服平台", PARENT + "\\" + PRODUCT]
    before_keys = {str(view): {key: registry(winreg.HKEY_CURRENT_USER, key, view) for key in keys} for view in VIEWS}
    before_machine = {str(view): {key: registry(winreg.HKEY_LOCAL_MACHINE, key, view) for key in keys} for view in VIEWS}
    if previous:
        for entries in before_keys.values():
            for entry in entries.values():
                if entry is not None:
                    values = entry["values"]
                    location = values.get("InstallLocation", values.get("", [""]))[0].strip('"')
                    assert Path(location).resolve() == app
        before_keys = previous["registry_before"]
    assert all(value is None for entries in before_keys.values() for value in entries.values())
    assert all(value is None for entries in before_machine.values() for value in entries.values())
    parent_before = {str(view): registry(winreg.HKEY_CURRENT_USER, PARENT, view) for view in VIEWS}
    parent_baseline_known = previous is None or "parent_before" in previous
    if previous and parent_baseline_known:
        parent_before = previous["parent_before"]
    run_before = {}
    for view in VIEWS:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN, 0, winreg.KEY_READ | view) as key:
            try:
                winreg.QueryValueEx(key, PRODUCT)
                raise AssertionError("Existing autostart value; abort test")
            except FileNotFoundError:
                run_before[str(view)] = None
    shortcut = known_folder(2) / (PRODUCT + ".lnk")
    desktop = known_folder(16) / (PRODUCT + ".lnk")
    assert shortcut.resolve() == Path(plan["start_menu_shortcut"]).resolve()
    if previous:
        assert shortcut.exists() and shortcut_target(shortcut) == (app / "mc-server-hub.exe").resolve()
    else:
        assert not shortcut.exists()
    assert not desktop.exists()
    formal = Path(os.environ["APPDATA"]) / "MCServerHub"
    fingerprints = lambda: {n: sha(formal / n) if (formal / n).exists() else None for n in ("hub_config.json", "config.json", "hub_config.1x-backup.json")}
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    report = {"plan": str(args.plan), "workspace": str(workspace), "checks": [], "formal_before": previous["formal_before"] if previous else fingerprints(), "registry_before": before_keys, "parent_baseline_known": parent_baseline_known}
    if parent_baseline_known:
        report["parent_before"] = parent_before
    save = lambda: (EVIDENCE / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    def passed(message):
        report["checks"].append(message)
        print("PASS", message, flush=True)
        save()
    command = "([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"
    elevated = subprocess.check_output(["powershell.exe", "-NoProfile", "-Command", command]).decode().strip()
    report["parent_elevated"] = elevated
    report["phase"] = "preflight passed; about to install"
    save()
    env = {**os.environ, **plan["environment"]}
    try:
        if previous:
            report["installer_exit"] = previous["installer_exit"]
            report["resumed_without_reinstallation"] = True
        else:
            result = subprocess.run([plan["installer"], "/S", "/D=" + str(app)], env=env, timeout=120)
            report["installer_exit"] = result.returncode
            assert result.returncode == 0
        exe = app / "mc-server-hub.exe"
        assert exe.is_file() and (app / "uninstall.exe").is_file()
        report["payload_comparison"] = support.verify_nsis_payload(exe.read_bytes(), (ROOT / "target/release/mc-server-hub.exe").read_bytes())
        report["installed_exe_sha256"] = sha(exe)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.txt", "使用說明.txt", "THIRD_PARTY_LICENSES.txt", "SBOM.cdx.json"):
            assert (app / name).is_file()
            expected = ROOT / ("docs/使用說明.txt" if name == "使用說明.txt" else "target/release-support/" + name if name in ("THIRD_PARTY_LICENSES.txt", "SBOM.cdx.json") else name)
            assert sha(app / name) == sha(expected), name
        assert shortcut.is_file() and shortcut_target(shortcut) == exe.resolve()
        assert not desktop.exists()
        installed_keys = {str(view): {key: registry(winreg.HKEY_CURRENT_USER, key, view) for key in keys} for view in VIEWS}
        report["registry_installed"] = installed_keys
        entry = installed_keys[str(VIEWS[0])][keys[0]]
        assert entry["values"]["DisplayVersion"][0] == "2.0.0"
        assert Path(entry["values"]["InstallLocation"][0].strip('"')).resolve() == app
        assert {str(view): {key: registry(winreg.HKEY_LOCAL_MACHINE, key, view) for key in keys} for view in VIEWS} == before_machine
        report["phase"] = "installed"
        passed("安裝到核准TEMP位置、exe僅差預期NSIS標記且附件hash相符；開始功能表正確、無桌面捷徑、HKLM未改")
        ui = subprocess.run(["node", str(ROOT / "tests/installed-app-ui.cjs"), str(args.plan.resolve()), str(EVIDENCE / "ui")], env=env, timeout=120)
        assert ui.returncode == 0, "Installed app UI failed; preserve installation for diagnosis"
        report["ui"] = json.loads((EVIDENCE / "ui/report.json").read_text(encoding="utf-8"))
        assert not report["ui"].get("cleanup")
        before_data, before_world, before_server = tree(data), tree(server / "world"), tree(server)
        report["data_before_uninstall"] = before_data
        report["world_before_uninstall"] = before_world
        report["installed_files"] = tree(app)
        assert before_world == {"level.dat": plan["fixture_sha256_before"]["fake-server\\world\\level.dat"]}
        assert fingerprints() == report["formal_before"]
        # All destructive work stays inside the approved app folder and the two known new HKCU keys.
        assert app.resolve().is_relative_to(workspace) and app.name == "app"
        assert registry(winreg.HKEY_CURRENT_USER, keys[1], VIEWS[0])["values"][""][0].strip('"').lower() in (str(app).lower(), str(Path(plan["install_dir"])).lower())
        report["phase"] = "about to uninstall"
        save()
        result = subprocess.run([str(app / "uninstall.exe"), "/S"], env=env, timeout=120)
        report["uninstaller_exit"] = result.returncode
        assert result.returncode == 0
        end = time.monotonic() + 40
        while time.monotonic() < end and (app.exists() or registry(winreg.HKEY_CURRENT_USER, keys[0], VIEWS[0]) is not None):
            time.sleep(.2)
        assert not exe.exists() and not (app / "uninstall.exe").exists(), "Installer-owned files not fully removed; no broad cleanup"
        assert not app.exists(), "Install directory still exists; preserve unexpected contents for review"
        assert not shortcut.exists() and not desktop.exists()
        assert tree(data) == before_data and tree(server) == before_server
        report["data_after_uninstall"], report["world_after_uninstall"] = tree(data), tree(server / "world")
        passed("解除程式完成；測試設定、啟動檔及假世界逐檔SHA-256完全保留")
        # The manufacturer preference key intentionally survives normal uninstall; restore the absent test baseline only.
        def delete_owned_key(path, view):
            assert path == keys[1] or path.startswith(keys[1] + "\\")
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path, 0, winreg.KEY_READ | view) as key:
                children = [winreg.EnumKey(key, i) for i in range(winreg.QueryInfoKey(key)[0])]
            for child in children:
                delete_owned_key(path + "\\" + child, view)
            winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, path, view, 0)
        for view in VIEWS:
            assert registry(winreg.HKEY_CURRENT_USER, keys[0], view) is None
            current = registry(winreg.HKEY_CURRENT_USER, keys[1], view)
            if current is not None:
                assert current == installed_keys[str(view)][keys[1]], "Registry changed concurrently; preserve it"
                delete_owned_key(keys[1], view)
            parent = registry(winreg.HKEY_CURRENT_USER, PARENT, view)
            if parent_baseline_known and parent_before[str(view)] is None and parent == {"values": {}, "children": {}}:
                winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, PARENT, view, 0)
        report["registry_after"] = {str(view): {key: registry(winreg.HKEY_CURRENT_USER, key, view) for key in keys} for view in VIEWS}
        assert report["registry_after"] == before_keys
        if parent_baseline_known:
            assert {str(view): registry(winreg.HKEY_CURRENT_USER, PARENT, view) for view in VIEWS} == parent_before
        else:
            report["parent_cleanup"] = "Original publisher-container baseline not captured on first attempt; preserved the empty container rather than delete uncertain existing state. Both test product keys removed."
        assert {str(view): {key: registry(winreg.HKEY_LOCAL_MACHINE, key, view) for key in keys} for view in VIEWS} == before_machine
        assert tree(data) == before_data and tree(server) == before_server
        report["formal_after"] = fingerprints()
        assert report["formal_after"] == report["formal_before"]
        report["app_directory_removed"] = not app.exists()
        report["run_after"] = {str(view): run_value(view) for view in VIEWS}
        assert report["run_after"] == run_before
        report["phase"] = "complete"
        passed("只清除本次新增產品登記；兩個產品鍵、Run基線與正式APPDATA三檔一致，fixture留存；未知父鍵保留")
    except BaseException as error:
        report["failure"] = str(error)
        save()
        raise
    save()
    print("COMPLETE", len(report["checks"]), "installation checks and", len(report["ui"]["checks"]), "UI checks")


if __name__ == "__main__":
    main()
