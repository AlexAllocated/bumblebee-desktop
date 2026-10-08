#!/usr/bin/env python3
"""Collect the exact notices shipped with resolved Cargo and JavaScript packages."""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "src-tauri/resources/third-party-licenses.txt"


def notices(directory):
    paths = []
    for path in directory.iterdir():
        if path.is_file() and path.name.lower().startswith(("license", "licence", "copying", "notice", "copyright")):
            paths.append(path)
    result = []
    for path in sorted(paths):
        try:
            result.append((path.name, path.read_text(encoding="utf-8-sig")))
        except UnicodeError:
            raise RuntimeError(f"License file is not text: {path.name}") from None
    return result


def main():
    supplements = json.loads((ROOT / "licenses/supplemental.json").read_text())
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT
    ))
    sections = []
    missing = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] in metadata["workspace_members"]:
            continue
        directory = Path(package["manifest_path"]).parent
        entries = notices(directory)
        supplement = supplements.get(f"crate:{package['name']}@{package['version']}")
        if not entries and supplement:
            entries = [(Path(name).name, (ROOT / name).read_text(encoding="utf-8-sig")) for name in supplement["files"]]
        if package.get("license_file"):
            path = directory / package["license_file"]
            if path.name not in {name for name, _ in entries}:
                entries.append((path.name, path.read_text()))
        identifier = f"Rust: {package['name']} {package['version']}"
        if not entries:
            missing.append(identifier)
        sections.append((identifier, package.get("license") or "See source license", package.get("repository") or package.get("source") or "Vendored source", entries))

    seen = set()
    modules = ROOT / "node_modules"
    # Bun installs package contents under .bun and links the public module tree.
    for directory, subdirs, filenames in os.walk(modules, followlinks=False):
        subdirs[:] = [name for name in subdirs if name not in {".cache", ".vite", ".bin"}]
        if "package.json" not in filenames:
            continue
        path = Path(directory)
        try:
            package = json.loads((path / "package.json").read_text())
        except (ValueError, UnicodeError):
            continue
        name, version = package.get("name"), package.get("version")
        if not name or not version or (name, version) in seen:
            continue
        seen.add((name, version))
        entries = notices(path)
        identifier = f"JavaScript: {name} {version}"
        if not entries:
            missing.append(identifier)
        repository = package.get("repository", "")
        if isinstance(repository, dict):
            repository = repository.get("url", "")
        sections.append((identifier, str(package.get("license", "See source license")), repository, entries))

    lines = ["Bumblebee dependency license notices", "", "Includes resolved build/test dependencies as well as application dependencies.", ""]
    for filename in ("LICENSE", "ASSETS.md", "THIRD_PARTY_NOTICES.md"):
        lines.extend([f"===== Bumblebee: {filename} =====", (ROOT / filename).read_text(), ""])
    for path in sorted((ROOT / "crates/audio/native").iterdir()):
        if path.is_file() and any(word in path.name.lower() for word in ("license", "notice", "redist")):
            # Microsoft's NuGet license is Windows-1252; retain its characters.
            encoding = "cp1252" if path.name == "azure-LICENSE.md" else "utf-8-sig"
            lines.extend([f"===== Native: {path.name} =====", path.read_text(encoding=encoding), ""])
    for identifier, license_id, repository, entries in sorted(sections):
        lines.extend([f"===== {identifier} =====", f"Declared license: {license_id}", f"Source: {repository}", ""])
        for name, body in entries:
            lines.extend([f"--- {name} ---", body, ""])
        if not entries:
            lines.append("No standalone license document was included in the dependency archive; see its declared license and source above.\n")
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("\n".join(lines))
    print(f"Collected notices for {len(sections)} resolved packages into {OUTPUT.relative_to(ROOT)}")
    if missing:
        print("Dependencies without a standalone notice file:")
        print("\n".join(missing))


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"License collection failed: {error}", file=sys.stderr)
        sys.exit(1)
