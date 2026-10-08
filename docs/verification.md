# Verification status

This document describes observed results, not release promises.

The desktop application is in development preview. Local core, rendering, native startup and one packaged NixOS smoke test have passed. Clean Windows/Ubuntu installation, live streaming sessions and public-release acceptance remain separate gates.

Implementation is being verified in this order: local persistence and action recovery; packaged desktop/native dependencies; integrated audio and rendering; provider connections and chat customization; retained agent behavior; public preview and upgrade behavior.

Azure retirement evidence and the private source snapshot are stored outside this public repository. Production data is intentionally not being migrated.

## Core behavior

The final local workspace run passed 81 core tests, 44 audio tests and three desktop transport tests (three opt-in native audio tests were skipped in this ordinary run). Coverage includes persistent chatter assignments, catalog resolution, provider authorization ordering, message deduplication, image download/approval boundaries, complete speech-cache entries and expiration, SQLite upgrades, scoped memories/reminders, and the native agent's confirmation/cancellation/uncertain-action ledger. Queue regressions verify that global cancellation invalidates already queued turns, actor cancellation remains isolated, and stopped sessions cannot enqueue work into a replacement session. See [agent behavior](agent-architecture.md) for the tested contracts and provider boundaries. These tests do not establish a live streaming session.

Follow-up regressions cover restart-safe Twitch/YouTube-to-Discord replies, changed linked identities, third-party recipient rejection, silent dashboard prompts and original-requester cancellation. Trusted dashboard answers neither create chatter profiles nor emit overlay chat/speech. Cached WAV reuse renews the disposable overlay file before another clip prunes old media; publication and pruning stay serialized even if the calling task is canceled. The regression verifies that reused bytes remain available while an unused expired clip is removed.

## Live native Speech check

The opt-in `speech_probe` example ran against Azure using the real bundled native SDK and Bumblebee Buddy preset. It returned 671,162 bytes of PCM WAV audio, a 6,990 ms audio clock and 11 ordered word boundaries. Repeating the request returned exact cached audio and timing without reading the provider credential again. A pre-canceled request never reached provider dispatch. This check did not connect Discord or post anything to a streaming platform; it does not establish audio-device output or OBS synchronization.

## Live native agent check

The opt-in `agent_probe` ran against OpenAI using `gpt-5.4-mini`, after verifying model access with the saved OS-keyring credential. OpenAI accepted the complete 67-tool function catalog and the strict final-response schema. The shared native turn loop completed a greeting in two Responses requests and one local delivery-configuration call, taking 4,405 ms. It consumed 13,109 input tokens and 91 output tokens. The final reply and assistant history persisted, and the reply was verified after reopening the isolated SQLite database.

The probe started no application engine, audio or streaming connections. Its executor boundary rejected all tools except silent local delivery configuration, and final delivery wrote only to temporary local storage. It created no artifacts or outward messages. The ordinary 74 core tests passed with the probe feature enabled; an additional focused guard test verified rejection of external tools, speech, public progress and Discord delivery. The default feature-free core also compiled. This proves the tested Responses contract and short turn, not live managed research/image/code work or external platform effects. See [probe instructions](agent-architecture.md#opt-in-live-responses-probe).

## Desktop foundation on NixOS

Observed locally on 2026-10-07:

- Svelte/TypeScript checking passed with zero errors or warnings; the production Vite build passed.
- 31 renderer/frontend tests passed, including preserved model download/retry, geometry, edge and projection behavior, viewport-safe speech bubbles, long-caption scrolling and exact caption punctuation.
- The original animated Bumblebee model rendered in the browser using the new shared stage.
- The native Tauri shell compiled on NixOS with GTK 3.24.52 and WebKitGTK 2.54.1. Three transport tests passed, covering loopback Host validation, capability tokens, path traversal, rendering-only routes and revocation.
- The built debug executable launched in the desktop session and its isolated `--smoke-test` passed: SQLite persistence after reopening, actual OS-keyring write/read/delete, actual Speech SDK and TEN VAD native-library loading, and loopback renderer/token checks. The extended smoke check also fetched bundled JavaScript, CSS, GLB and puppet assets, verified a real WebSocket's initial layout and event delivery, and confirmed that token rotation closed the existing socket. No provider credentials or production application data were used by that smoke test.

The optimized Linux build also produced a 160.48 MiB Debian package. Its extracted executable passed the smoke test using its packaged native resources and embedded renderer assets on NixOS. This local package links against Nix libraries and was tested in the pinned runtime environment; it is not an Ubuntu installer distribution artifact.

This does not yet establish clean-machine installer acceptance, Windows behavior, live provider permissions, Discord audio, full provider-to-OBS synchronization, tray interaction, autostart, or upgrade preservation. The installer workflow includes installed-package smoke tests on Windows and Ubuntu; passing them must be verified before publishing a preview.

## Native release packaging

The focused provider/native review also passed 14 provider tests and 44 audio tests. New cases cover an incoming app-authored event arriving before its HTTP receipt, duplicate echoes, rejected sends preserving real owner chat, cancellation during acknowledgement, ambiguous YouTube broadcast discovery, and delayed Discord permission responses completing after revocation or channel changes. The two native-resource tests are opt-in and are not counted in the 44 ordinary audio tests.

Eight packaging safeguard tests passed (including installed Visual Studio version selection and square AppImage icons): native architecture rejection, required Windows CRT files, Linux dependency closure and missing-library failures, installed package alternatives/providers, exact resource manifests, and missing copyright failures. An isolated Ubuntu 24.04 build compiled the actual Speech wrapper, collected 23 native runtime library files with relative loader paths, and preserved notices for 297 installed runtime packages. Every packaged native library then resolved in a separate pristine Ubuntu container without development packages. This proves the tested native loader dependency closure, not the complete AppImage or GUI installation. Actual Windows CRT loading and both CI installer smoke tests remain to be observed.

The explicit packaged native-lifecycle regression also passed locally on NixOS. It constructed TEN VAD, processed synthetic zero PCM, initialized the packaged Speech wake-word model, verified recognition remained pending on silence, and closed both engines. It opened no audio device or provider connection. Installed smoke tests run the same lifecycle off the async worker with a 30-second deadline and an enclosing process timeout; this exercises delayed native dependencies as well as library exports.

## Installed interface rendering

A fresh debug Debian package was built after the shared renderer readiness check was added. Its extracted executable passed the stricter smoke under Xvfb on NixOS: the packaged Svelte interface mounted, the real Bumblebee GLB loaded and Babylon completed a rendered frame before native, storage, asset and WebSocket checks completed. A subsequent check verified nontransparent model pixels and captured the complete native dashboard with the original animated bee. Xvfb requires software rendering and disabled DMABUF; those settings are confined to the isolated Linux CI helper and are not imposed on normal application startup. This establishes actual native webview rendering in the tested Nix environment. CI applies the installed check to the Windows installer, Linux Debian package and Linux AppImage.

## OBS Browser Source on NixOS

An isolated OBS Studio 32.1.2 instance rendered the unchanged production overlay assets through Browser Source at 1280×720 using the workstation's native NVIDIA graphics stack. An authenticated OBS WebSocket connection controlled only a temporary profile and scene. That scene contained only the browser source; browser audio was routed into OBS with monitoring disabled. No stream, microphone capture or desktop-audio capture was started. Original OBS configuration files were checksum-identical afterward.

The recorded 22.4-second H.264/AAC output contained the animated Bumblebee and word-highlighted captions, with 48 kHz stereo audio. The local fixture used previously generated speech and its word timings, requiring no new provider request. Four speech events fetched audio through the test server. The captured audio had nonzero speech (approximately −24 dBFS RMS) and exact zero PCM during the checked silence windows after both `stop_speech` and WebSocket disconnection. Across 553 OBS volume-meter events, residual audio ended within approximately 162 ms of cancellation/disconnection. The overlay reconnected approximately one second after disconnection and played a subsequent greeting. OBS reported streaming inactive with zero transmitted bytes.

This check used the real compiled renderer and an isolated HTTP/WebSocket fixture server, not the running Tauri application's transport or a live provider/Discord session. Those integration boundaries remain separate. It exposed a speech-bubble viewport-clipping issue at 1280×720. The renderer now constrains bubbles to horizontal and vertical viewport gutters, preserves original punctuation, and scrolls long captions within the bubble to follow the highlighted word. Seven focused regression tests cover captions up to 6,000 characters, small viewports, resizing and backward seeks; Svelte checking and the production build pass. The corrected overlay has not been visually rerun in OBS yet. Under Xvfb, the installed OBS browser's CEF 127 GPU process crashed with both default software graphics and explicit software ANGLE; native graphics succeeded. OBS also exited with a segmentation-fault status when the temporary instance was terminated after its recording finalized. These OBS environment/shutdown limitations are not treated as successful headless acceptance.
