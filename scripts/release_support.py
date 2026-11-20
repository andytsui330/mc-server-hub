"""Local packaging support. No downloads, installation, deletion, signing-key reads or publishing."""
import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import tomllib
from urllib.parse import quote
import uuid

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target/release-support"
FIXED_WINDOWS = "MCServerHub-Setup-Windows.exe"
NSIS_EXE_SHA1 = "6d45d4e8ccf18fe1fdac9164058f356102a2ecf2"
PLUGIN_SHA1 = "75197fee3c6a814fe035788d1c34ead39349b860"
REQUIRED_NSIS = ["NSIS.exe", "makensis.exe", "Bin/makensis.exe", "Stubs/lzma-x86-unicode",
                 "Stubs/lzma_solid-x86-unicode", "Plugins/x86-unicode/additional/nsis_tauri_utils.dll",
                 "Include/MUI2.nsh", "Include/FileFunc.nsh", "Include/x64.nsh", "Include/nsDialogs.nsh",
                 "Include/WinMessages.nsh", "Include/Win/COM.nsh", "Include/Win/Propkey.nsh", "Include/Win/RestartManager.nsh"]


def load_json(file):
    data = Path(file).read_bytes()
    return json.loads(data.decode("utf-16" if data.startswith(b"\xff\xfe") else "utf-8-sig"))


def digest(file, algorithm="sha256"):
    with Path(file).open("rb") as stream:
        return hashlib.file_digest(stream, algorithm).hexdigest()


def release_notes(changelog, version):
    match = re.search(r"^## " + re.escape(version) + r"(?:[（( ].*)?$", changelog, re.M)
    if not match:
        raise ValueError(f"CHANGELOG has no exact version {version}")
    rest = changelog[match.end():]
    return re.split(r"^## ", rest, maxsplit=1, flags=re.M)[0].strip() + "\n"


UNRELEASED_WORDS = ("未發佈", "尚未正式", "候選", "準備中", "預定")


def release_doc_problems(root, version):
    """發版把關：程式不得寫死版本、CHANGELOG 該版要有日期、對外文件不得留發佈前的字樣。"""
    root = Path(root)
    problems = []
    literal = re.compile(r"(?<![\d.])" + re.escape(version) + r"(?![\d.])")
    for file in sorted((root / "app/src").rglob("*")):
        if file.suffix in {".ts", ".tsx", ".css", ".html"} and literal.search(file.read_text(encoding="utf-8")):
            problems.append(f"app/src 寫死版本號 {version}：{file.relative_to(root).as_posix()}（改讀 getVersion()）")
    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if not re.search(r"^## " + re.escape(version) + r"[（(]\d{4}-\d{2}-\d{2}[）)][ \t]*\r?$", changelog, re.M):
        problems.append(f"CHANGELOG 標題要寫成「## {version}（YYYY-MM-DD）」")
    docs = {"README.md": (root / "README.md").read_text(encoding="utf-8"),
            "docs/使用說明.txt": (root / "docs/使用說明.txt").read_text(encoding="utf-8-sig")}
    try:
        docs[f"CHANGELOG {version} 段落"] = release_notes(changelog, version)
    except ValueError:
        pass
    for name, text in docs.items():
        problems.extend(f"{name} 還有「{word}」" for word in UNRELEASED_WORDS if word in text)
    return problems


def checksum_lines(files):
    files = sorted(map(Path, files), key=lambda p: p.name)
    if len({p.name for p in files}) != len(files):
        raise ValueError("duplicate artifact names")
    return "".join(f"{digest(file)}  {file.name}\n" for file in files)


def verify_nsis_payload(installed, unbundled):
    """Tauri packages NSS then restores the build exe to UNK; no other byte may differ."""
    if installed == unbundled:
        return "identical"
    old, new = b"__TAURI_BUNDLE_TYPE_VAR_UNK", b"__TAURI_BUNDLE_TYPE_VAR_NSS"
    if unbundled.count(old) != 1 or installed.count(new) != 1 or installed != unbundled.replace(old, new):
        raise ValueError("Installed payload differs outside the unique Tauri NSIS marker")
    return "UNK->NSS"


def nsis_preflight(directory):
    """Inspect only: the Tauri bundler must not repair/download tools without approval."""
    directory = Path(directory)
    problems = [f"missing {name}" for name in REQUIRED_NSIS if not (directory / name).is_file()]
    for name, expected in [("NSIS.exe", NSIS_EXE_SHA1),
                           ("Plugins/x86-unicode/additional/nsis_tauri_utils.dll", PLUGIN_SHA1)]:
        file = directory / name
        if file.is_file() and digest(file, "sha1") != expected:
            problems.append(f"unexpected hash: {name}")
    return problems


def validate_config(config):
    bundle = config["bundle"]
    nsis = bundle["windows"]["nsis"]
    if bundle["targets"] != ["nsis"] or nsis["installMode"] != "currentUser":
        raise ValueError("Windows must use current-user NSIS only")
    if bundle.get("createUpdaterArtifacts") is not True:
        raise ValueError("production updater signatures must remain enabled")
    if nsis.get("languages") != ["TradChinese", "English"]:
        raise ValueError("installer languages must be explicit")
    if not nsis.get("template") or not bundle.get("resources"):
        raise ValueError("installer template and legal resources are required")
    if config["plugins"]["updater"].get("requireSignedVersion") is not True:
        raise ValueError("updater version signature verification must remain enabled")


def license_texts(directory, extras):
    directory = Path(directory).resolve()
    files = []
    # 不分大小寫：Windows 檔案系統本來就不分，macOS 的 glob 會分（例：objc2 系列的 license-mit）。
    entries = list(directory.iterdir())
    for pattern in ("license*", "licence*", "copying*", "notice*", "copyright*"):
        files.extend(e for e in entries if e.is_file() and fnmatch.fnmatchcase(e.name.lower(), pattern))
    for folder in (e for e in entries if e.is_dir() and e.name.lower() == "licenses"):
        files.extend(file for file in folder.iterdir() if file.is_file())
    files.extend(map(Path, extras))
    if not files:
        raise ValueError(f"missing full license text: {directory.name}")
    result = []
    for file in sorted(set(files)):
        if file.is_symlink() or file.stat().st_size > 1048576:
            raise ValueError("unsafe license input")
        result.append(file.name + "\n" + file.read_text(encoding="utf-8-sig"))
    return "\n\n".join(result)


def expression(value):
    value = value.strip()
    if "/" in value:
        value = " OR ".join(part.strip() for part in value.split("/"))
    return value


def merge_metadata(parts):
    """Union of per-target `cargo metadata` resolves (universal macOS = x86_64 + aarch64)."""
    packages, nodes = {}, {}
    for metadata in parts:
        for pkg in metadata["packages"]:
            packages.setdefault(pkg["id"], pkg)
        for node in metadata["resolve"]["nodes"]:
            merged = nodes.setdefault(node["id"], {"id": node["id"], "deps": []})
            by_pkg = {d["pkg"]: d for d in merged["deps"]}
            for dep in node["deps"]:
                if dep["pkg"] not in by_pkg:
                    by_pkg[dep["pkg"]] = {"pkg": dep["pkg"], "dep_kinds": []}
                    merged["deps"].append(by_pkg[dep["pkg"]])
                for kind in dep["dep_kinds"]:
                    if kind not in by_pkg[dep["pkg"]]["dep_kinds"]:
                        by_pkg[dep["pkg"]]["dep_kinds"].append(kind)
    return {"packages": list(packages.values()), "resolve": {"nodes": list(nodes.values())}}


SBOM_SERIAL_NAMESPACE = uuid.UUID("6f1d7c3e-2a4b-5c8d-9e0f-1a2b3c4d5e6f")


def with_serial_number(bom):
    """CycloneDX的serialNumber：由內容算出的uuid5，同內容永遠同序號（可重現）。"""
    body = {k: v for k, v in bom.items() if k != "serialNumber"}
    digest = json.dumps(body, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    serial = "urn:uuid:" + str(uuid.uuid5(SBOM_SERIAL_NAMESPACE, digest))
    ordered = {}
    for key, value in body.items():
        ordered[key] = value
        if key == "specVersion":
            ordered["serialNumber"] = serial
    return ordered


def prepare(metadata_file=None, target="x86_64-pc-windows-msvc"):
    targets = [target] if isinstance(target, str) else list(target)
    target = ",".join(targets)
    config = load_json(ROOT / "app/src-tauri/tauri.conf.json")
    validate_config(config)
    version = config["version"]
    for file in (ROOT / "core/Cargo.toml", ROOT / "app/src-tauri/Cargo.toml"):
        if tomllib.loads(file.read_text(encoding="utf-8"))["package"]["version"] != version:
            raise ValueError("Rust version mismatch")
    if load_json(ROOT / "app/package.json")["version"] != version:
        raise ValueError("frontend version mismatch")
    problems = release_doc_problems(ROOT, version)
    if problems:
        raise ValueError("release docs not ready:\n" + "\n".join(problems))
    if metadata_file:
        metadata = load_json(metadata_file)
    else:
        cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo.exe")
        parts = [json.loads(subprocess.run([cargo, "metadata", "--locked", "--offline", "--format-version", "1",
                                            "--filter-platform", triple], cwd=ROOT, check=True, capture_output=True).stdout)
                 for triple in targets]
        metadata = parts[0] if len(parts) == 1 else merge_metadata(parts)
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {p["id"]: p for p in metadata["resolve"]["nodes"]}
    root_id = next(p["id"] for p in packages.values() if p["name"] == "mc-server-hub" and not p["source"])
    seen, pending = set(), [root_id]
    edges = {}
    while pending:
        identity = pending.pop()
        if identity in seen:
            continue
        seen.add(identity)
        edges[identity] = [d["pkg"] for d in nodes[identity]["deps"] if any(k["kind"] != "dev" for k in d["dep_kinds"])]
        pending.extend(edges[identity])
    sources = load_json(ROOT / "packaging/license-sources.json")
    components, notices = [], [(ROOT / "THIRD_PARTY_NOTICES.txt").read_text(encoding="utf-8")]
    refs = {identity: f"pkg:cargo/{packages[identity]['name']}@{packages[identity]['version']}" for identity in seen}
    for identity in sorted(seen, key=lambda i: refs[i]):
        pkg = packages[identity]
        extras = []
        for entry in sources.get(pkg["name"], []):
            file = ROOT / entry["file"]
            if digest(file) != entry["sha256"]:
                raise ValueError(f"upstream license changed: {file.name}")
            extras.append(file)
        if pkg.get("license_file"):
            extras.append(Path(pkg["manifest_path"]).parent / pkg["license_file"])
        if not pkg["source"]:
            extras.append(ROOT / "LICENSE")
        full = license_texts(Path(pkg["manifest_path"]).parent, extras)
        source = f"https://crates.io/api/v1/crates/{pkg['name']}/{pkg['version']}/download" if pkg["source"] else "https://github.com/andytsui330/mc-server-hub"
        notices.append(f"===== {pkg['name']} {pkg['version']} ({pkg['license']}) =====\nSource: {source}\n\n{full}")
        if identity != root_id:
            components.append({"type": "library", "name": pkg["name"], "version": pkg["version"],
                               "bom-ref": refs[identity], "purl": refs[identity],
                               "licenses": [{"expression": expression(pkg["license"])}]})
    dependencies = [{"ref": refs[i], "dependsOn": sorted(refs[d] for d in edges[i])} for i in sorted(seen)]
    lock = load_json(ROOT / "app/package-lock.json")
    npm_refs = {}
    for location, pkg in sorted(lock["packages"].items()):
        if not location or pkg.get("dev"):
            continue
        name = location.rsplit("node_modules/", 1)[1]
        ref = f"pkg:npm/{quote(name, safe='/')}@{pkg['version']}"
        npm_refs[name] = ref
        full = license_texts(ROOT / "app" / location, [])
        notices.append(f"===== {name} {pkg['version']} ({pkg['license']}) =====\n\n{full}")
        components.append({"type": "library", "name": name, "version": pkg["version"], "bom-ref": ref,
                           "purl": ref, "licenses": [{"expression": expression(pkg["license"])}]})
    for location, pkg in sorted(lock["packages"].items()):
        if not location or pkg.get("dev"):
            continue
        name = location.rsplit("node_modules/", 1)[1]
        dependencies.append({"ref": npm_refs[name], "dependsOn": sorted(npm_refs[n] for n in pkg.get("dependencies", {}) if n in npm_refs)})
    root_edges = next(d for d in dependencies if d["ref"] == refs[root_id])
    root_edges["dependsOn"].extend(npm_refs.values())
    for name in ("Inter Tight", "Geist Mono"):
        ref = "embedded-font:" + name.replace(" ", "-")
        components.append({"type": "file", "name": name, "bom-ref": ref, "licenses": [{"license": {"id": "OFL-1.1"}}]})
        root_edges["dependsOn"].append(ref)
        dependencies.append({"ref": ref, "dependsOn": []})
    bom = {"$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json", "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
           "metadata": {"component": {"type": "application", "name": "MC Server Hub", "version": version, "bom-ref": refs[root_id]},
                        "lifecycles": [{"phase": "pre-build"}],
                        "properties": [{"name": "mchub:target", "value": target},
                                       {"name": "mchub:scope", "value": "resolved Rust build/runtime and npm production dependency inventory; not binary reachability proof"}]},
           "components": components, "dependencies": dependencies}
    bom = with_serial_number(bom)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "SBOM.cdx.json").write_text(json.dumps(bom, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (OUTPUT / "THIRD_PARTY_LICENSES.txt").write_text("\n\n".join(notices) + "\n", encoding="utf-8")
    (OUTPUT / "RELEASE-NOTES.md").write_text(release_notes((ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), version), encoding="utf-8")
    print(json.dumps({"version": version, "rust_packages": len(seen), "npm_packages": len(npm_refs), "components": len(components)}, ensure_ascii=False))


BUILD_PROFILES = {
    # 本機隔離測試：不裝WebView2、不產更新簽章。
    "local-test": ("src-tauri/tauri.local-test.conf.json",
                   "Unsigned installer candidate for isolated testing. Not a release or signed updater.\n"),
    # GitHub打包：WebView2照正式設定自動補裝；只關掉打包時的更新簽章（私鑰不上雲，發佈時在本機另簽）。
    "ci-release": ("src-tauri/tauri.ci-release.conf.json",
                   "Unsigned release candidate built by GitHub Actions (WebView2 bootstrapper kept). "
                   "Updater signature is created separately at release time.\n"),
}


def validate_ci_release_config(override):
    if override != {"bundle": {"createUpdaterArtifacts": False}}:
        raise ValueError("ci-release config may only disable build-time updater artifacts")


def stage_windows(installer, output, status=BUILD_PROFILES["local-test"][1]):
    installer, output = Path(installer).resolve(), Path(output).resolve()
    if not installer.is_file() or installer.suffix.lower() != ".exe" or installer.read_bytes()[:2] != b"MZ":
        raise ValueError("expected an existing Windows installer")
    if output.exists() and any(output.iterdir()):
        raise ValueError("use a new empty staging directory; existing release artifacts are preserved")
    output.mkdir(parents=True, exist_ok=True)
    dest = output / FIXED_WINDOWS
    shutil.copy2(installer, dest)
    for name in ("SBOM.cdx.json", "THIRD_PARTY_LICENSES.txt", "RELEASE-NOTES.md"):
        shutil.copy2(OUTPUT / name, output / name)
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.txt"):
        shutil.copy2(ROOT / name, output / name)
    (output / "BUILD-STATUS.txt").write_text(status, encoding="utf-8")
    (output / "SHA256SUMS.txt").write_text(checksum_lines([f for f in output.iterdir() if f.is_file()]), encoding="utf-8")
    print(f"staged {FIXED_WINDOWS}; no publishing or updater manifest generated")


def build_windows_test(allow_tool_download=False, profile="local-test"):
    if profile not in BUILD_PROFILES:
        raise ValueError(f"unknown build profile: {profile}")
    config_path, status = BUILD_PROFILES[profile]
    if profile == "ci-release":
        validate_ci_release_config(json.loads((ROOT / "app" / config_path).read_text(encoding="utf-8")))
    directory = Path(os.environ.get("LOCALAPPDATA", "")) / "tauri/NSIS"
    problems = nsis_preflight(directory)
    if problems and directory.exists():
        raise ValueError("Existing NSIS cache is incomplete or changed; preserved. Review repair/deletion separately.")
    if problems and not allow_tool_download:
        raise ValueError("NSIS 3.13 and helper 0.5.3 require approval before tool download/installation; no builder invoked")
    if sys.platform != "win32":
        raise ValueError("Windows installer must be built on Windows")
    prepare()
    npm = shutil.which("npm")
    if not npm:
        raise ValueError("npm is not installed")
    subprocess.run([npm, "run", "tauri", "--", "build", "--ci", "--no-sign", "--bundles", "nsis",
                    "--config", config_path], cwd=ROOT / "app", check=True)
    installers = list((ROOT / "target/release/bundle/nsis").glob("*.exe"))
    if len(installers) != 1:
        raise ValueError("expected exactly one NSIS installer; preserve old artifacts and review instead of guessing")
    stage_windows(installers[0], ROOT / "target" / f"windows-candidate-{time.time_ns()}", status)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--metadata", type=Path)
    prepare_parser.add_argument("--target", action="append",
                                help="Repeat for universal macOS: --target x86_64-apple-darwin --target aarch64-apple-darwin (default: x86_64-pc-windows-msvc)")
    check_parser = commands.add_parser("check-tools")
    check_parser.add_argument("--nsis-dir", type=Path, default=Path(os.environ.get("LOCALAPPDATA", "")) / "tauri/NSIS")
    stage_parser = commands.add_parser("stage-windows")
    stage_parser.add_argument("--installer", required=True, type=Path)
    stage_parser.add_argument("--output", required=True, type=Path)
    build_parser = commands.add_parser("build-windows-test")
    build_parser.add_argument("--allow-tool-download", action="store_true", help="Only after explicit approval; an existing incomplete cache is never removed")
    build_parser.add_argument("--profile", choices=sorted(BUILD_PROFILES), default="local-test",
                              help="local-test: isolated test build; ci-release: GitHub Actions release candidate")
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.metadata, args.target or ["x86_64-pc-windows-msvc"])
    elif args.command == "check-tools":
        problems = nsis_preflight(args.nsis_dir)
        print(json.dumps({"directory": str(args.nsis_dir), "nsis": "3.13", "plugin": "0.5.3", "problems": problems}, ensure_ascii=False, indent=2))
        return 2 if problems else 0
    elif args.command == "stage-windows":
        stage_windows(args.installer, args.output)
    else:
        build_windows_test(args.allow_tool_download, args.profile)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"STOP: {error}", file=sys.stderr)
        sys.exit(1)
