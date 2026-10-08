# Verification status

This document describes observed results, not release promises.

The desktop application is in development preview. Local core, rendering, native startup and one packaged NixOS smoke test have passed. Clean Windows/Ubuntu installation, live streaming sessions and public-release acceptance remain separate gates.

Implementation is being verified in this order: local persistence and action recovery; packaged desktop/native dependencies; integrated audio and rendering; provider connections and chat customization; retained agent behavior; public preview and upgrade behavior.

Azure retirement evidence and the private source snapshot are stored outside this public repository. Production data is intentionally not being migrated.

## Core behavior

The final local workspace run passed 74 core tests, 44 audio tests and three desktop transport tests (two opt-in native audio tests were skipped in this ordinary run). Coverage includes persistent chatter assignments, catalog resolution, provider authorization ordering, message deduplication, image download/approval boundaries, complete speech-cache entries and expiration, SQLite upgrades, scoped memories/reminders, and the native agent's confirmation/cancellation/uncertain-action ledger. Queue regressions verify that global cancellation invalidates already queued turns, actor cancellation remains isolated, and stopped sessions cannot enqueue work into a replacement session. See [agent behavior](agent-architecture.md) for the tested contracts and provider boundaries. These tests do not establish a live streaming session.

## Live native Speech check

The opt-in `speech_probe` example ran against Azure using the real bundled native SDK and Bumblebee Buddy preset. It returned 671,162 bytes of PCM WAV audio, a 6,990 ms audio clock and 11 ordered word boundaries. Repeating the request returned exact cached audio and timing without reading the provider credential again. A pre-canceled request never reached provider dispatch. This check did not connect Discord or post anything to a streaming platform; it does not establish audio-device output or OBS synchronization.

## Desktop foundation on NixOS

Observed locally on 2026-10-07:

- Svelte/TypeScript checking passed with zero errors or warnings; the production Vite build passed.
- 24 renderer/frontend tests passed, including preserved model download/retry, geometry, edge and projection behavior.
- The original animated Bumblebee model rendered in the browser using the new shared stage.
- The native Tauri shell compiled on NixOS with GTK 3.24.52 and WebKitGTK 2.54.1. Three transport tests passed, covering loopback Host validation, capability tokens, path traversal, rendering-only routes and revocation.
- The built debug executable launched in the desktop session and its isolated `--smoke-test` passed: SQLite persistence after reopening, actual OS-keyring write/read/delete, actual Speech SDK and TEN VAD native-library loading, and loopback renderer/token checks. The extended smoke check also fetched bundled JavaScript, CSS, GLB and puppet assets, verified a real WebSocket's initial layout and event delivery, and confirmed that token rotation closed the existing socket. No provider credentials or production application data were used by that smoke test.

The optimized Linux build also produced a 160.48 MiB Debian package. Its extracted executable passed the smoke test using its packaged native resources and embedded renderer assets on NixOS. This local package links against Nix libraries and was tested in the pinned runtime environment; it is not an Ubuntu installer distribution artifact.

This does not yet establish clean-machine installer acceptance, Windows behavior, live provider permissions, Discord audio, OBS capture, tray interaction, autostart, or upgrade preservation. The installer workflow includes installed-package smoke tests on Windows and Ubuntu; passing them must be verified before publishing a preview.

## Native release packaging

The focused provider/native review also passed 14 provider tests and 44 audio tests. New cases cover an incoming app-authored event arriving before its HTTP receipt, duplicate echoes, rejected sends preserving real owner chat, cancellation during acknowledgement, ambiguous YouTube broadcast discovery, and delayed Discord permission responses completing after revocation or channel changes. The two native-resource tests are opt-in and are not counted in the 44 ordinary audio tests.

Six packaging safeguard tests passed: native architecture rejection, required Windows CRT files, Linux dependency closure and missing-library failures, installed package alternatives/providers, exact resource manifests, and missing copyright failures. An isolated Ubuntu 24.04 build compiled the actual Speech wrapper, collected 23 native runtime library files with relative loader paths, and preserved notices for 297 installed runtime packages. Every packaged native library then resolved in a separate pristine Ubuntu container without development packages. This proves the tested native loader dependency closure, not the complete AppImage or GUI installation. Actual Windows CRT loading and both CI installer smoke tests remain to be observed.
