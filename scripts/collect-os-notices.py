#!/usr/bin/env python3
"""Preserve installed Ubuntu runtime-package notices and exact source versions.

Collect GTK/WebKit, native audio, GStreamer plugins and their installed dependency
closure. The package index describes the builder's inputs, including runtime
dependencies that AppImage may deliberately leave to the base OS.
"""
from pathlib import Path
import argparse
import json
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MARKER = "===== Linux distribution runtime notices ====="
RUNTIME_PREFIXES = (
    "libgtk-3-", "libwebkit2gtk-4.1-", "libjavascriptcoregtk-4.1-", "libsoup-3.0-",
    "libgstreamer", "libgst", "gstreamer1.0-", "libgdk-pixbuf-2.0-", "librsvg2-",
    "glib-networking", "libsecret-1-", "libayatana-", "libdbusmenu-", "libssl3",
    "libasound2", "libuuid1", "libc++", "libstdc++6", "libgcc-s1", "libnss3",
    "libnspr4", "libdconf1", "dconf-service", "adwaita-icon-theme", "hicolor-icon-theme",
    "shared-mime-info",
)


def relation_names(relation: str):
    names = []
    for alternative in relation.split("|"):
        match = re.match(r"\s*([a-z0-9][a-z0-9+.-]*)(?::([a-z0-9-]+))?", alternative)
        if match:
            names.append(match[1] if match[2] in (None, "any", "native") else f"{match[1]}:{match[2]}")
    return names


def parse_packages(text: str):
    packages = {}
    for line in text.splitlines():
        fields = line.split("\t")
        if len(fields) != 10 or not fields[9].startswith("ii"):
            continue
        binary, name, arch, version, source, source_version, depends, predepends, provides, _ = fields
        packages[binary] = dict(name=name, architecture=arch, version=version,
                                source=source or name, source_version=source_version or version,
                                depends=", ".join((depends, predepends)), provides=provides)
    return packages


def closure(packages: dict, seeds: set):
    providers = {}
    # Prefer native/all packages when satisfying an unqualified dependency.
    for binary, package in sorted(packages.items(), key=lambda item: item[1]["architecture"] not in ("amd64", "all")):
        providers.setdefault(package["name"], binary)
        providers[binary] = binary
        for relation in package["provides"].split(","):
            for name in relation_names(relation):
                providers.setdefault(name, binary)
    result, pending = set(), sorted(seeds)
    while pending:
        requested = pending.pop()
        binary = providers.get(requested)
        if not binary:
            raise RuntimeError(f"Runtime package is not installed: {requested}")
        if binary in result:
            continue
        result.add(binary)
        for relation in packages[binary]["depends"].split(","):
            names = relation_names(relation)
            if not names:
                continue
            # Preserve every installed alternative's notice, not an arbitrary
            # choice when two providers happen to be installed on the runner.
            installed = {providers[name] for name in names if name in providers}
            if not installed:
                raise RuntimeError(f"Unsatisfied installed-package dependency: {binary}: {relation}")
            pending.extend(sorted(installed))
    return sorted(result)


def collect(output: Path, docs: Path = Path("/usr/share/doc"), common: Path = Path("/usr/share/common-licenses")):
    fields = ("binary:Package", "Package", "Architecture", "Version", "source:Package", "source:Version", "Depends", "Pre-Depends", "Provides", "db:Status-Abbrev")
    format_string = "\t".join("${" + field + "}" for field in fields) + "\n"
    packages = parse_packages(subprocess.check_output(["dpkg-query", "-W", "-f=" + format_string], text=True))
    seeds = {binary for binary, p in packages.items()
             if p["name"].startswith(RUNTIME_PREFIXES) and not p["name"].endswith(("-dev", "-dbg", "-doc"))}
    if not any(packages[name]["name"].startswith("libwebkit2gtk-4.1-") for name in seeds):
        raise RuntimeError("WebKit runtime is missing; collect OS notices on the Linux release builder")
    origin = output / "native/linux-runtime-origin.json"
    if origin.exists():
        for entry in json.loads(origin.read_text())["distribution_dependencies"].values():
            seeds.update(entry["packages"])
    selected = closure(packages, seeds)
    lines = [MARKER, "", "Exact binary/source versions from the Ubuntu release builder.",
             "Ubuntu source archives: https://launchpad.net/ubuntu/+source/<source>/<source-version>", ""]
    for binary in selected:
        package = packages[binary]
        path = docs / package["name"] / "copyright"
        if not path.is_file():
            path = docs / binary / "copyright"
        if not path.is_file():
            raise RuntimeError(f"Runtime package has no installed copyright notice: {binary}")
        lines.extend([f"----- {binary} {package['version']} -----",
                      f"Source package: {package['source']} {package['source_version']}",
                      f"Source archive: https://launchpad.net/ubuntu/+source/{package['source']}/{package['source_version']}",
                      path.read_text(encoding="utf-8"), ""])
    # Debian copyright documents reference these files instead of inlining them.
    for path in sorted(common.iterdir()):
        if path.is_file():
            lines.extend([f"----- /usr/share/common-licenses/{path.name} -----", path.read_text(encoding="utf-8"), ""])
    license_file = output / "third-party-licenses.txt"
    base = license_file.read_text().split(MARKER, 1)[0].rstrip()
    license_file.write_text(base + "\n\n" + "\n".join(lines))
    print(f"Appended runtime notices for {len(selected)} installed distribution packages")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "src-tauri/resources")
    collect(parser.parse_args().output.resolve())
