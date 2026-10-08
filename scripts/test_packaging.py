from pathlib import Path
import importlib.util
import json
import struct
import tempfile
import unittest
from unittest.mock import patch
from native_packaging import assert_x64, select_windows_crt, windows_generator, windows_toolchain, write_manifest


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


linux = module("bundle-linux-native")
notices = module("collect-os-notices")


class PackagingTests(unittest.TestCase):
    def test_configured_appimage_png_icons_are_square(self):
        root = Path(__file__).resolve().parents[1] / "src-tauri"
        config = json.loads((root / "tauri.conf.json").read_text())
        icons = [root / name for name in config["bundle"]["icon"] if name.endswith(".png")]
        self.assertTrue(icons, "AppImage requires at least one PNG icon")
        for icon in icons:
            content = icon.read_bytes()
            self.assertEqual(content[:8], b"\x89PNG\r\n\x1a\n")
            width, height = struct.unpack(">II", content[16:24])
            self.assertEqual(width, height, f"AppImage requires a square icon: {icon.name}")
            self.assertGreaterEqual(width, 32)

    def test_windows_toolchain_follows_the_installed_2026_runner(self):
        data = json.dumps([{"installationPath": "C:/VS/2026", "installationVersion": "18.10.100"}])
        with patch("native_packaging.subprocess.check_output", return_value=data):
            self.assertEqual(windows_toolchain(), (Path("C:/VS/2026"), "Visual Studio 18 2026"))
        self.assertEqual(windows_generator(17), "Visual Studio 17 2022")
        with self.assertRaisesRegex(RuntimeError, "redistribution license"):
            windows_generator(19)

    def test_rejects_wrong_binary_architecture_before_shipping(self):
        with tempfile.TemporaryDirectory() as folder:
            library = Path(folder) / "runtime.dll"
            data = bytearray(256)
            data[:2] = b"MZ"
            struct.pack_into("<I", data, 0x3C, 128)
            data[128:132] = b"PE\0\0"
            struct.pack_into("<H", data, 132, 0x14C)
            library.write_bytes(data)
            with self.assertRaisesRegex(RuntimeError, "x64"):
                assert_x64(library)
            struct.pack_into("<H", data, 132, 0x8664)
            library.write_bytes(data)
            assert_x64(library)
            elf = bytearray(64)
            elf[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", elf, 18, 62)
            library.write_bytes(elf)
            assert_x64(library)
            struct.pack_into("<H", elf, 18, 183)
            library.write_bytes(elf)
            with self.assertRaisesRegex(RuntimeError, "x64"):
                assert_x64(library)

    def test_windows_selects_newest_x64_release_crt_and_requires_codecvt(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            required = ("msvcp140.dll", "msvcp140_codecvt_ids.dll", "vcruntime140.dll", "vcruntime140_1.dll")
            for version in ("14.9.1", "14.40.1"):
                for arch in ("x86", "x64"):
                    directory = root / version / arch / "Microsoft.VC143.CRT"
                    directory.mkdir(parents=True)
                    for filename in required:
                        (directory / filename).touch()
            selected = select_windows_crt(root)
            self.assertEqual(selected, root / "14.40.1/x64/Microsoft.VC143.CRT")
            (selected / "msvcp140_codecvt_ids.dll").unlink()
            with self.assertRaisesRegex(RuntimeError, "missing required"):
                select_windows_crt(root)

    def test_native_closure_does_not_bundle_glibc_but_catches_missing_ten_dependency(self):
        result = linux.dependencies("""
 linux-vdso.so.1 (0x00007fff)
 libc++.so.1 => /usr/lib/x86_64-linux-gnu/libc++.so.1 (0x00007fff)
 libc++abi.so.1 => /usr/lib/x86_64-linux-gnu/libc++abi.so.1 (0x00007fff)
 libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007fff)
 libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007fff)
 /lib64/ld-linux-x86-64.so.2 (0x00007fff)
""")
        self.assertEqual(set(result), {"libc++.so.1", "libc++abi.so.1"})
        with self.assertRaisesRegex(RuntimeError, "Missing native dependency"):
            linux.dependencies("libc++abi.so.1 => not found")

    def test_os_notice_closure_keeps_installed_alternatives_and_virtual_providers(self):
        def row(name, dependencies="", provides="", status="ii "):
            return "\t".join((name + ":amd64", name, "amd64", "1.2", name + "-source", "1.1", dependencies, "", provides, status))
        packages = notices.parse_packages("\n".join((
            row("libwebkit", "libc6 (>= 2.34), tls:any | ssl"), row("libc6"),
            row("glib-networking", "libc6", "tls (= 1)"), row("ssl"),
            row("removed", status="rc "),
        )))
        self.assertNotIn("removed:amd64", packages)
        self.assertEqual(notices.closure(packages, {"libwebkit"}), [
            "glib-networking:amd64", "libc6:amd64", "libwebkit:amd64", "ssl:amd64",
        ])
        packages["libwebkit:amd64"]["depends"] = "missing-runtime"
        with self.assertRaisesRegex(RuntimeError, "Unsatisfied"):
            notices.closure(packages, {"libwebkit"})

    def test_manifest_tracks_exact_windows_root_and_native_resources(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            for name in ("native/voice.so", "windows-runtime/msvcp140.dll", "keyword_models/wake.table"):
                path = output / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
            first = write_manifest(output)
            (output / "native/voice.so").write_bytes(b"changed")
            second = write_manifest(output)
            self.assertEqual(len(second), 3)
            self.assertIn("msvcp140.dll", second)
            self.assertNotEqual(first["native/voice.so"], second["native/voice.so"])
            self.assertEqual(json.loads((output / "native-manifest.json").read_text()), second)

    def test_missing_os_copyright_fails_instead_of_silent_notice_loss(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            row = "\t".join(("libwebkit2gtk-4.1-0:amd64", "libwebkit2gtk-4.1-0", "amd64", "1", "webkit2gtk", "1", "", "", "", "ii "))
            with patch.object(notices.subprocess, "check_output", return_value=row):
                with self.assertRaisesRegex(RuntimeError, "no installed copyright"):
                    notices.collect(output, output / "docs", output / "common")


if __name__ == "__main__":
    unittest.main()
