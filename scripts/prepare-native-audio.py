#!/usr/bin/env python3
"""Build the native Speech SDK bridge for the local Windows/Linux x64 target.

No credentials or provider connection are needed. The SDK archive is pinned and
checksummed; only its redistributable native files enter the application bundle.
Opus is built statically by Cargo. TEN VAD is a separately licensed binary with
additional upstream conditions; its exact license and pinned binary are bundled.
"""
from pathlib import Path
import argparse
import hashlib
import platform
import shutil
import subprocess
import urllib.request
import zipfile
from native_packaging import assert_x64, bundle_windows_crt, write_manifest

ROOT = Path(__file__).resolve().parents[1]
SDK_VERSION = "1.50.0"
SDK_SHA256 = "500e3f01b2a9c797a127a9ab3870718fa38abfc8441362d72c7ff0b34a6d9449"
SDK_URL = f"https://api.nuget.org/v3-flatcontainer/microsoft.cognitiveservices.speech/{SDK_VERSION}/microsoft.cognitiveservices.speech.{SDK_VERSION}.nupkg"

TEN_COMMIT = "22a3bcd4509d0faaa8eef4881e8af5f39c178950"
TEN_BINARIES = {
    "linux-x64": ("lib/Linux/x64/libten_vad.so", "5abfe6bf6e9a4fcea6b440240f0a9a0f431ab5006e48a4e16465ebe681ffd90f"),
    "win-x64": ("lib/Windows/x64/ten_vad.dll", "38937f5604fa93a7941db7b9326992b792fa3731ebf9353973b3234457c6064b"),
}


def prepare(output: Path):
    system = platform.system()
    if system not in ("Linux", "Windows") or platform.machine().lower() not in ("x86_64", "amd64"):
        raise SystemExit("Native audio packaging currently supports Windows and Linux x64.")
    target = "win-x64" if system == "Windows" else "linux-x64"
    work = ROOT / "target" / "native-audio" / target
    work.mkdir(parents=True, exist_ok=True)
    archive = work / f"speech-{SDK_VERSION}.nupkg"
    if not archive.exists():
        temp = archive.with_suffix(".download")
        with urllib.request.urlopen(SDK_URL, timeout=60) as response, temp.open("wb") as dest:
            shutil.copyfileobj(response, dest)
        temp.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SDK_SHA256:
        raise SystemExit(f"Speech SDK archive checksum mismatch: {archive}")
    sdk = work / "sdk"
    include = sdk / "include"
    lib = sdk / "lib" / "x64"
    lib.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as files:
        for name in files.namelist():
            if name.endswith("/"):
                continue
            if name.startswith("build/native/include/"):
                destination = include / name.removeprefix("build/native/include/")
            elif name.startswith(f"runtimes/{target}/native/"):
                destination = lib / Path(name).name
            elif system == "Windows" and name == "build/native/x64/Release/Microsoft.CognitiveServices.Speech.core.lib":
                destination = lib / Path(name).name
            else:
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(files.read(name))
    native_output = output / "native"
    # Generated output is replaceable. Never retain stale dependencies from a
    # previous SDK/version or cross-platform build.
    for folder in (native_output, output / "windows-runtime"):
        if folder.exists():
            shutil.rmtree(folder)
    native_output.mkdir(parents=True, exist_ok=True)
    build = work / "build"
    generator = ["-G", "Visual Studio 17 2022", "-A", "x64"] if system == "Windows" else []
    subprocess.run(["cmake", *generator, "-S", str(ROOT / "crates/audio/native"), "-B", str(build),
                    f"-DSPEECH_SDK_ROOT={sdk}", "-DCMAKE_BUILD_TYPE=Release",
                    f"-DCMAKE_INSTALL_PREFIX={native_output}"], check=True)
    subprocess.run(["cmake", "--build", str(build), "--config", "Release", "--parallel", "2"], check=True)
    subprocess.run(["cmake", "--install", str(build), "--config", "Release"], check=True)
    for library in lib.iterdir():
        if library.suffix in (".dll", ".so"):
            shutil.copy2(library, native_output / library.name)
    ten_path, ten_checksum = TEN_BINARIES[target]
    ten_binary = work / Path(ten_path).name
    if not ten_binary.exists():
        with urllib.request.urlopen(f"https://raw.githubusercontent.com/TEN-framework/ten-vad/{TEN_COMMIT}/{ten_path}", timeout=60) as response:
            ten_binary.write_bytes(response.read())
    if hashlib.sha256(ten_binary.read_bytes()).hexdigest() != ten_checksum:
        raise SystemExit("TEN VAD binary checksum mismatch")
    shutil.copy2(ten_binary, native_output / ten_binary.name)
    if system == "Windows":
        bundle_windows_crt(output)
    for library in native_output.iterdir():
        if library.suffix in (".dll", ".so"):
            assert_x64(library)
    shutil.copytree(ROOT / "crates/audio/resources/keyword_models", output / "keyword_models", dirs_exist_ok=True)
    for source in (ROOT / "crates/audio/native").iterdir():
        if not ("LICENSE" in source.name or "NOTICES" in source.name or source.name.startswith("azure-")): continue
        shutil.copy2(source, native_output / source.name)
    manifest = write_manifest(output)
    print(f"Prepared {len(manifest)} native audio resources in {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "src-tauri/resources")
    prepare(parser.parse_args().output.resolve())
