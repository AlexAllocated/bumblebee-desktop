#!/usr/bin/env python3
"""Bundle the dlopen-only audio dependency closure on a Debian/Ubuntu release runner.

AppImage discovery starts at the executable and can miss resource libraries.
Keep these dependencies beside the native entry points; never bundle glibc or
its loader. Nix source builds use the pinned flake environment instead.
"""
from pathlib import Path
import argparse
import json
import os
import re
import shutil
import subprocess
from native_packaging import assert_x64, write_manifest

ROOT = Path(__file__).resolve().parents[1]
SYSTEM_ABI = re.compile(r"^(?:ld-linux.*|lib(?:c|m|dl|pthread|rt|resolv|util|anl)\.so\..*)$")


def dependencies(output: str):
    result = {}
    for line in output.splitlines():
        if "=> not found" in line:
            raise RuntimeError(f"Missing native dependency: {line.strip()}")
        match = re.match(r"\s*(\S+)\s+=>\s+(/.+?)\s+\(0x[0-9a-f]+\)", line)
        if match and not SYSTEM_ABI.fullmatch(match[1]):
            result[match[1]] = Path(match[2])
    return result


def owners(path: Path):
    found = set()
    for candidate in {path, path.resolve()}:
        lookup = subprocess.run(["dpkg-query", "-S", str(candidate)], text=True, capture_output=True)
        for line in lookup.stdout.splitlines():
            if ": /" in line:
                found.update(line.split(": /", 1)[0].split(", "))
    if not found:
        raise RuntimeError(f"No distribution package owns native dependency {path}")
    return sorted(found)


def bundle(output: Path):
    if not shutil.which("dpkg-query") or not shutil.which("patchelf"):
        raise RuntimeError("Linux release bundling requires a Debian/Ubuntu runner with dpkg-query and patchelf")
    native = output / "native"
    original = sorted(native.glob("*.so"))
    if not original:
        raise RuntimeError("Run prepare-native-audio.py before Linux release bundling")
    environment = dict(os.environ, LD_LIBRARY_PATH=str(native))
    pending = list(original)
    # The Speech SDK loads these at runtime rather than listing them in NEEDED.
    cache = subprocess.check_output(["ldconfig", "-p"], text=True)
    for soname in ("libssl.so.3", "libcrypto.so.3", "libasound.so.2"):
        matches = re.findall(r"^\s*" + re.escape(soname) + r"\s+\([^\n]*x86-64[^\n]*\)\s+=>\s+(\S+)$", cache, re.M)
        if not matches:
            raise RuntimeError(f"Required runtime library is not installed: {soname}")
        pending.append(Path(matches[0]))
    copied, visited = {}, set()
    while pending:
        path = pending.pop()
        if path in visited:
            continue
        visited.add(path)
        assert_x64(path)
        if path.parent != native:
            destination = native / path.name
            if not destination.exists():
                shutil.copy2(path, destination)
                copied[path.name] = {"source": str(path), "packages": owners(path)}
        else:
            destination = path
        resolved = dependencies(subprocess.check_output(["ldd", str(path)], text=True, env=environment))
        for soname, source in sorted(resolved.items()):
            target = native / soname
            if not target.exists():
                assert_x64(source)
                shutil.copy2(source, target)
                copied[soname] = {"source": str(source), "packages": owners(source)}
            pending.append(source)
    # Microsoft binaries already use $ORIGIN. Preserve their shipped bytes.
    # TEN needs a relative runpath; record that packaging modification explicitly.
    patched = sorted({"libten_vad.so", "libbumblebee_speech_wrapper.so", *copied})
    for name in patched:
        subprocess.run(["patchelf", "--set-rpath", "$ORIGIN", str(native / name)], check=True)
    # Check the installed loader layout without development LD_LIBRARY_PATH.
    # Every non-glibc dependency must resolve from the application resources.
    clean_environment = dict(os.environ)
    clean_environment.pop("LD_LIBRARY_PATH", None)
    for path in original + [native / name for name in copied]:
        resolved = dependencies(subprocess.check_output(["ldd", str(path)], text=True, env=clean_environment))
        for soname, source in resolved.items():
            if source.parent.resolve() != native.resolve():
                raise RuntimeError(f"Native dependency escaped the bundle: {path.name}: {soname}")
    (native / "linux-runtime-origin.json").write_text(json.dumps({
        "distribution_dependencies": copied,
        "runpath_changed_to_origin": patched,
        "base_system": "Ubuntu 24.04 or newer compatible glibc; glibc and its loader are not bundled",
    }, indent=2, sort_keys=True) + "\n")
    write_manifest(output)
    print(f"Bundled {len(copied)} distro runtime libraries for native audio")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "src-tauri/resources")
    bundle(parser.parse_args().output.resolve())
