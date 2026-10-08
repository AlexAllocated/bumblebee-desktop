# Desktop builds

The installer workflow builds Windows x64 NSIS and Linux x64 Debian/AppImage artifacts. It builds the native Speech SDK wrapper, bundles native resources, and runs a smoke test against the installed executable. Preview artifacts are unsigned and carry SHA-256 checksums. There is no updater, hosted authentication relay, deployment workflow, or paid signing dependency.

## Development

Install Rust, Bun, Python 3 and CMake. Linux also needs the Tauri GTK3/WebKit 4.1 development libraries. On NixOS, use `nix develop` from this repository; the flake pins the development dependencies.

```sh
bun install --frozen-lockfile
python scripts/prepare-native-audio.py
python scripts/collect-licenses.py
bun run build
bun run desktop
```

The first production build can take several minutes. `bun tauri build --bundles nsis` builds Windows; `bun tauri build --bundles deb,appimage` builds Linux. A Linux AppImage should be built on the oldest supported Linux runner (currently Ubuntu 24.04). NixOS uses the flake development environment for source builds and `appimageTools` for released AppImages, not a generic unwrapped AppImage.

Windows builds require Visual Studio 2022 or 2026 C++ tools and release x64 redistributables. Native preparation discovers the installed toolchain rather than assuming the runner's Visual Studio generation. Native preparation validates the binary architecture and includes the VC runtime beside both the executable and Speech wrapper. It does not rely on Visual Studio or a separately installed VC runtime on the viewer's machine. These app-local libraries receive security updates through new Bumblebee installers.

On the Ubuntu release builder, run `python scripts/bundle-linux-native.py` after native preparation, then `python scripts/collect-os-notices.py` after the ordinary license collector. The workflow does this automatically. This explicitly includes TEN's C++ runtime and the Speech SDK's dynamic dependencies, because an AppImage dependency scan can miss libraries loaded from resource paths. Native libraries use `$ORIGIN`; the host still supplies the base glibc ABI. The generated package notices preserve installed copyright documents, common license texts, and exact source-package versions. The notices are build evidence, not a claim that redistribution/source obligations for every bundled dependency have received a legal review.

The renderer build is also used by the local OBS server during desktop development. Run `bun run build` after changing renderer code to update OBS; the desktop window itself reloads through Vite.

## NixOS installation

After a preview has been published, pin its AppImage URL and SHA-256 in your Nix configuration:

```nix
let
  bumblebee = inputs.bumblebee-desktop.lib.packageAppImage {
    version = "0.1.0";
    url = "https://github.com/AlexAllocated/bumblebee-desktop/releases/download/v0.1.0/Bumblebee_0.1.0_amd64.AppImage";
    sha256 = "REPLACE_WITH_RELEASE_HASH";
  };
in { environment.systemPackages = [ bumblebee ]; }
```

Use the actual release asset filename. This example does not claim that the release exists yet. Enable a Secret Service implementation such as GNOME Keyring in your desktop session, and a StatusNotifier tray to retain access while streaming with the window closed.

## Installation and upgrade checks

`bumblebee-desktop --smoke-test` tests the installed package using temporary SQLite and keyring entries, checks native library loading, fetches packaged assets and exercises WebSocket delivery and revocation through loopback. The native webview must load the Svelte interface, load the bee model and render a frame within 60 seconds. Linux CI runs this against both the installed Debian executable and the AppImage (using its extraction runtime, without requiring FUSE). It exits with a nonzero status on failure. This does not replace visual inspection or test live provider permissions, Discord voice, OBS capture, or a complete streaming session.

The Windows CI smoke runs from an empty temporary working directory with a PATH containing only standard Windows directories, so it cannot find native resources through the checkout or development-tool paths. Its installer and application processes have bounded timeouts, and application output is captured in the job log. The hosted Windows runner still has development tools and system runtimes installed; this is installed-package evidence, not a pristine Windows-machine acceptance claim.

Install an upgrade over the existing version and verify settings, profiles, memories, reminders and approved images remain available. Data lives in the OS application-data directory (`buzz.bumblebee.desktop`); secrets live in the OS keyring. Uninstalling or upgrading the executable must not migrate credentials into plain files.

Check `docs/verification.md` for the actual tested boundaries before calling a build a release candidate.
