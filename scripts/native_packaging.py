"""Shared native-release checks; no provider credentials or host configuration."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess


def assert_x64(path: Path):
    data = path.read_bytes()
    if data.startswith(b"MZ"):
        offset = struct.unpack_from("<I", data, 0x3C)[0]
        valid = data[offset:offset + 4] == b"PE\0\0" and struct.unpack_from("<H", data, offset + 4)[0] == 0x8664
    elif data.startswith(b"\x7fELF"):
        valid = data[4:6] == b"\x02\x01" and struct.unpack_from("<H", data, 18)[0] == 62
    else:
        valid = False
    if not valid:
        raise RuntimeError(f"Expected an x64 native library: {path.name}")


def write_manifest(output: Path):
    manifest = {str(p.relative_to(output / folder if folder == "windows-runtime" else output)).replace("\\", "/"): hashlib.sha256(p.read_bytes()).hexdigest()
                for folder in ("native", "keyword_models", "windows-runtime")
                for p in sorted((output / folder).rglob("*")) if p.is_file()}
    # AppImage's linuxdeploy can rewrite ELF search-path tags after this stage.
    # Preserve input provenance without claiming these are final installer hashes.
    document = {
        "schemaVersion": 1,
        "stage": "prepared-before-tauri-bundling",
        "pathBase": "packaged resource directory (Windows CRT paths are installation-root relative)",
        "note": "Tauri bundlers may rewrite ELF loader metadata. These are prepared-resource digests, not final installed-file digests. Verify released installers with SHA256SUMS.txt.",
        "resources": manifest,
    }
    (output / "native-manifest.json").write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    return manifest


def select_windows_crt(redist: Path) -> Path:
    candidates = []
    for version in redist.iterdir():
        if not re.fullmatch(r"\d+(?:\.\d+)+", version.name):
            continue
        for directory in version.glob("x64/Microsoft.VC*.CRT"):
            candidates.append((tuple(map(int, version.name.split("."))), directory))
    if not candidates:
        raise RuntimeError("Visual Studio x64 release CRT redistributables are missing")
    directory = max(candidates)[1]
    names = {path.name.lower() for path in directory.glob("*.dll")}
    required = {"msvcp140.dll", "msvcp140_codecvt_ids.dll", "vcruntime140.dll", "vcruntime140_1.dll"}
    if not required <= names:
        raise RuntimeError("Visual Studio release CRT is missing required Speech SDK DLLs")
    return directory


def windows_toolchain():
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installations = json.loads(subprocess.check_output([
        str(vswhere), "-latest", "-products", "*",
        "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-format", "json", "-utf8",
    ], encoding="utf-8"))
    if not installations:
        raise RuntimeError("Visual Studio C++ tools are required to package Windows x64")
    installation = installations[0]
    major = int(installation["installationVersion"].split(".")[0])
    return Path(installation["installationPath"]), windows_generator(major)


def windows_generator(major: int):
    year = {17: 2022, 18: 2026}.get(major)
    if year is None:
        raise RuntimeError(f"Unsupported Visual Studio {major}; validate its toolchain and redistribution license first")
    return f"Visual Studio {major} {year}"


def bundle_windows_crt(output: Path, installation: Path, generator: str):
    directory = select_windows_crt(installation / "VC/Redist/MSVC")
    application = output / "windows-runtime"
    application.mkdir(parents=True, exist_ok=True)
    for library in sorted(directory.glob("*.dll")):
        assert_x64(library)
        # The executable imports the CRT before Rust runs; the native loader also
        # searches beside the Speech wrapper using LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR.
        shutil.copy2(library, application / library.name)
        shutil.copy2(library, output / "native" / library.name)
    (output / "native/msvc-runtime-origin.txt").write_text(
        f"{generator} release redistributables\n"
        f"Version: {directory.parent.parent.name}\nArchitecture: x64\n"
        "Source: VC/Redist/MSVC/<version>/x64/Microsoft.VC*.CRT\n"
        f"License: {'msvc-2026-RUNTIME-LICENSE.txt' if generator.endswith('2026') else 'msvc-RUNTIME-LICENSE.txt'}\n",
        encoding="utf-8",
    )
