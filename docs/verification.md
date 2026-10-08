# Verification status

This document describes observed results, not release promises.

The desktop application is an unsigned development preview. Current and historical evidence are separated below. Live streaming sessions, interactive tray/autostart behavior and cross-platform upgrade coverage remain separate verification boundaries.

Azure retirement evidence and the private source snapshot are stored outside this public repository. Production data is intentionally not being migrated.

## Dashboard and settings restoration — 2026-10-08

Local source checks passed: 109 core tests, 44 audio tests (four explicit native tests ignored), six desktop tests, and 29 interface/editor tests. Svelte checking reported zero errors and warnings. Nine packaging safeguard tests passed. New coverage includes sparse settings saves across overlapping refreshes, strict settings validation, atomic scoped resets, schema-4 upgrade preservation, per-platform audience policies, audio routing/mixing, owner-only caption cancellation, and private dashboard memory access after owner identity changes.

A private SQLite backup of the existing installation was migrated through the real storage API on a separate copy. Every pre-existing installation setting, the OBS capability token, other stored settings, and every pre-existing column and row outside the intentionally migrated layout/settings were preserved. The live database was not changed by this migration check.

The dark dashboard restores editing for Bumblebee, ordinary chat puppets and the separate streamer caption bubble. Its Settings drawer includes named Discord resources, audio/AI/platform policies, searchable viewer overrides, memory/reminder management and confirmed resets. Discord participant puppets and promotional controls are removed. Automatic Discord channel following, follow-up listening and platform event announcements are not implemented or exposed as working settings in this preview.

## Shared rendering packages — 2026-10-08 source revision

The current dashboard rewrite restores `@hivetech/bumblebee` and `@hivetech/speech-bubbles` as reusable workspace packages. The retained actor, animation, nameplate, SVG bubble, pagination and frame-rendering implementations now accept application-prepared audio. Hosted bot authentication, provider clients, demo credentials, Discord voice puppets and promotional celebrations are absent from these packages.

`bun run check:packages` and `bun run build:packages` passed. The Bumblebee package passed 148 tests; speech-bubbles passed 42. Ten new lifecycle tests cover bounded speech queuing, active/queued interruption, late preparation/model/decode completion, completion-triggered enqueue, hidden subjects, SSML-safe word mapping, and caption timing from the AudioContext clock rather than elapsed wall time. These tests use controlled browser/audio substitutes and do not play audio or call providers.

Both actual package tarballs were installed into an independent temporary consumer, without workspace links. The consumer explicitly pinned both unpublished tarballs, including the transitive speech-bubbles dependency, so the existing npm `0.0.4` release could not substitute for the restored source. Every public source/distribution export and MIT license was present. TypeScript checked all public entry points with `skipLibCheck: false` and no Node ambient types in both default and development conditions; browser bundling also passed in both conditions. This found and fixed a Node-specific timer declaration and missing source CSS declarations. No npm release was published.

The license collector was regenerated from the current resolved dependency tree and produced identical output on a second run. Its generated resource includes the upstream MIT license text and copyright for `immer 11.1.21` and `uuid 14.0.2`; each workspace package includes the project MIT license. The existing packaging workflow regenerates dependency notices, then Linux OS-runtime notices, before bundling. The local resource is not a substitute for the platform-specific notice collection and final installer inspection.

The historical preview, native-webview and OBS sections below describe `26b3731`, whose renderer preceded this package restoration. Current installer results are recorded separately; the earlier OBS capture does not establish actual OBS playback or provider-to-overlay synchronization for the restored renderer.

## Preview 2 installers — 2026-10-08

[Installer run 37751866939](https://github.com/AlexAllocated/bumblebee-desktop/actions/runs/37751866939) passed for source `69e95224ba55c189decfafa60d3e4143e1d8003f`, application version 0.1.1. Windows and Linux both passed package/interface checks, renderer tests and native tests. All three installed smoke tests passed: Windows NSIS, Linux Debian and Linux AppImage. They loaded the restored Svelte/package renderer and verified visible bee-model pixels, SQLite persistence, temporary OS-keyring operations, Speech/TEN VAD lifecycle, and the loopback assets/WebSocket/token-revocation contract. They did not start a streaming session or call a provider.

The exact CI AppImage also passed the full smoke through the repository's Nix wrapper on the maintainer's NixOS workstation, under isolated Xvfb without development-library paths. Its SHA-256 is `67f1cd96457593ece4452ab4408c8052444b2f5880771f9c7a0a0127828fa6b3`, also pinned in [packaging](packaging.md#nixos-installation). The native Nix source build passed the same smoke separately; that source-built Debian file was not published.

All installer checksums matched the CI artifact manifests. Archive inspections found the required project, artwork, dependency and native notices, including the new Immer/UUID notices. No environment, database, keyring, private-key or private-resume files were found. Windows matched all 48 preparation-resource hashes. AppImage/Deb executable differences were confined to ELF linkage metadata and Tauri's bundle-type marker; executable code sections matched. These are bounded inventory/pattern checks, not a repeated comparison against every historical credential value.

The workstation's declarative dotfiles pin was updated from Preview 1 to Preview 2. `nix flake check --all-systems`, the complete system build, and activation all passed. The actual installed launcher passed the isolated smoke, then the ordinary application reopened against its existing data. SQLite upgraded from schema 3 to 4; every pre-existing installation setting and the OBS token matched a fresh private backup. All six configured credential entries remained in the OS store. This installation had no chatter/history/memory/reminder rows yet; populated migration behavior is covered separately by the core tests. The authenticated local rendering endpoint and its JavaScript/CSS assets returned successfully. No session was started; live integrations remain deferred. The independent resume and tunnel services stayed active.

## Restored dashboard browser check — 2026-10-08

The real Svelte dashboard and shared package renderer were exercised in Chrome against the isolated fixture at source revision `69e9522`. From the repository root, after installing dependencies, run:

```sh
bunx vite --config tests/ui/vite.config.ts
```

Open `http://127.0.0.1:1422/` for the dashboard and `http://127.0.0.1:1422/overlay.html` for the rendering-only fixture. This explicitly injected test backend stores fixture settings in browser local storage under `bumblebee-dashboard-ui-fixture-v2`; it does not use the application's SQLite database or OS credentials. Connection badges and account records are fixture data. Speech controls use locally generated silent PCM and supplied word timings, with `audible: false`. No provider request, platform connection, microphone, native audio device or actual OBS instance is involved.

Observed checks:

- Moving Bumblebee and resizing from a corner retained its camera-facing actor behavior. Reloading restored the same editor bounds. Changing the streamer bubble theme, moving its camera target and resizing that target persisted; the rendering-only tab received those saved settings.
- The original package rendered the animated Bee, a Dandy chat puppet, its Twitch icon/nameplate and progressively revealed speech bubble. A 6,000-character dashboard speech preview advanced through bounded caption pages; the observed content's scroll height equaled its client height rather than overflowing vertically.
- An injected owner-caption event displayed the supplied text. The empty-text revocation event removed it. This exercises renderer handling of core events, not live Discord consent or transcription.
- Switching the renderer fixture between 960×540 and 800×600 kept Bumblebee correctly positioned and facing the camera. This found and fixed an engine-aspect update missing before responsive placement. Renderer startup passed the actual nontransparent-model-pixel readiness check; resized frames were inspected visually.
- With both editor preview flags saved as enabled, the rendering-only view showed neither a sample puppet nor the static streamer placeholder. Real chat speech displayed one chatter puppet, and revoking the real caption left no placeholder. The dashboard retained its editing previews. This found and fixed editor-only samples leaking into the broadcast view.
- Contextual overlay controls were readable in the dark dashboard. There were three editable subjects: Bumblebee, ordinary chat puppets and the separate streamer bubble; no Discord participant-puppet controls were present.

For a focused repeat, enable the chat and streamer previews in the dashboard, move and resize their frames, change a bubble theme, reload, then use the rendering fixture's speech/caption/revocation and viewport controls. The dashboard's Test speech field accepts the long-caption case without synthesizing speech. Keep the tab foregrounded for visual timing checks; background-tab throttling is not evidence of an active renderer stall.

The focused renderer suite passed 22 tests with 80 assertions, and Svelte checking reported zero errors or warnings after the last browser fixes. This browser run establishes the tested editor and renderer behavior with explicit local events. It does not establish the installed Tauri webview, its HTTP/WebSocket transport, audible timing, actual OBS Browser Source, or live provider-to-overlay synchronization for this revision. The historical OBS recording below predates this package restoration and is not a substitute for those checks.

## Core behavior

The final source checks passed on both Windows and Linux: 87 core tests, 44 audio tests and three desktop transport tests. Native-resource tests require explicit opt-in; there are four on Linux and three on Windows. Coverage includes persistent chatter assignments, catalog resolution, provider authorization ordering, message deduplication, image download/approval boundaries, complete speech-cache entries and expiration, SQLite upgrades, scoped memories/reminders, and the native agent's confirmation/cancellation/uncertain-action ledger. Queue regressions verify that global cancellation invalidates already queued turns, actor cancellation remains isolated, and stopped sessions cannot enqueue work into a replacement session. See [agent behavior](agent-architecture.md) for the tested contracts and provider boundaries. These tests do not establish a live streaming session.

Follow-up regressions cover restart-safe Twitch/YouTube-to-Discord replies, changed linked identities, third-party recipient rejection, silent dashboard prompts and original-requester cancellation. Trusted dashboard answers neither create chatter profiles nor emit overlay chat/speech. Cached WAV reuse renews the disposable overlay file before another clip prunes old media; publication and pruning stay serialized even if the calling task is canceled. The regression verifies that reused bytes remain available while an unused expired clip is removed.

Agent-off regressions cover new and queued voice captures, transcription dispatch, late results, ordinary chat and pending answers. Saved questions remain pending until the agent is re-enabled; local cancellation remains available. Spoken prompts explain the required wake phrase. Voice answers tolerate that address and sentence punctuation while retaining exact-choice precedence and rejecting compound or ambiguous approval; ordinary chat is not normalized this way.

## Preview installers

[Installer run 37730995943](https://github.com/AlexAllocated/bumblebee-desktop/actions/runs/37730995943) passed on Windows 2025 and Ubuntu 24.04 for source revision `26b3731b5082a51a06854c2d4a8af4d4d87ebdd9`. It produced the Windows x64 NSIS installer, Linux x64 Debian package and Linux x64 AppImage. All three installed checks passed: the packaged Svelte interface rendered visible pixels from the actual Bumblebee model, SQLite survived reopening, the OS keyring completed a temporary write/read/delete, TEN VAD processed synthetic silence, the packaged Speech keyword model initialized and closed, and loopback assets/WebSocket delivery/token revocation worked. No provider session or audio device was opened by these checks.

The Windows executable ran from an empty working directory with only standard Windows directories in its PATH. The hosted runner still has development tools and system runtimes installed; this is installed-package evidence, not a pristine Windows VM test.

The exact final AppImage also passed the complete installed check in an independent Ubuntu 24.04 runtime with network access disabled and no WebKit, libc++, compiler, Rust, Node or Bun packages installed. No preloaded library or development-library path was supplied. That same file passed through the repository's pinned Nix wrapper on the maintainer's NixOS workstation, under an isolated Xvfb display with no development-library environment injected. The package loaded its own native resources and rendered the real interface; the Nix wrapper's known Tauri mount-location warning is explained in [packaging](packaging.md#nixos-installation).

The final Debian package passed fresh installation and same-version reinstallation in a separate Ubuntu runtime. A fixture created through the real storage API retained settings, stable chatter profiles, memories, a future reminder, pending confirmations and approved image bytes. Its database and saved image remained byte-identical after reinstalling. This proves the tested reinstall path; the first preview has no earlier public version with which to establish a real version-to-version upgrade.

The Windows artifact's 48 preparation-stage resource hashes match the installed bytes. Both Linux packages retain a manifest explicitly labeled as preparation-stage provenance: Debian matches all 53 entries; AppImage matches 52, with its bundler changing one ELF `DT_RPATH` tag to `DT_RUNPATH` in the keyword runtime. Final download integrity is defined by the release's `SHA256SUMS.txt`, not by treating that intermediate manifest as a final-package checksum.

Archive inspection verified project, artwork and native/dependency notices and found no environment files, databases, keyring data or private-key files in the installers. Exact comparisons against 17 legacy credential values found no UTF-8 or UTF-16LE matches in any installer. Native library architectures match the x64 targets. Asset attribution, including Alopex's notice, remains bundled separately from the project-code MIT license.

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

These early source-built Nix checks are development evidence; distributable installer checks are recorded above. They do not establish live provider permissions, Discord audio, full provider-to-OBS synchronization, tray interaction, autostart, or version-to-version upgrade preservation.

## Native release packaging

The focused provider/native review also passed 14 provider tests and 44 audio tests. New cases cover an incoming app-authored event arriving before its HTTP receipt, duplicate echoes, rejected sends preserving real owner chat, cancellation during acknowledgement, ambiguous YouTube broadcast discovery, and delayed Discord permission responses completing after revocation or channel changes. Native-resource tests are opt-in and are not counted in the 44 ordinary audio tests.

Eight packaging safeguard tests passed (including installed Visual Studio version selection and square AppImage icons): native architecture rejection, required Windows CRT files, Linux dependency closure and missing-library failures, installed package alternatives/providers, preparation-stage resource manifests, and missing copyright failures. Linux also passed the real SDK cancellation regression, for nine packaging tests; that extra test is skipped on Windows. An isolated Ubuntu 24.04 build compiled the actual Speech wrapper, collected 23 native runtime library files with relative loader paths, and preserved notices for 297 installed runtime packages. Every packaged native library resolved in a separate pristine Ubuntu container without development packages. The final installed checks additionally exercised Windows CRT loading and both Linux package formats.

The explicit packaged native-lifecycle regression also passed locally on NixOS. It constructed TEN VAD, processed synthetic zero PCM, initialized the packaged Speech wake-word model, verified recognition remained pending on silence, and closed both engines. It opened no audio device or provider connection. Installed smoke tests run the same lifecycle off the async worker with a 30-second deadline and an enclosing process timeout; this exercises delayed native dependencies as well as library exports.

AppImage validation found that the bundler's duplicate Speech core could load from a directory without its keyword extensions. The audio library now loads the exact packaged core beside those extensions before loading the wrapper. A fresh-process regression puts a duplicate core first in the library search path and verifies successful keyword startup and shutdown. A separate Python regression deliberately loads a core without its extensions, observes the real SDK cancellation, and verifies that repeated polls and writes retain the original error without restarting recognition or hanging shutdown. Both regressions passed locally using synthetic silence, with no provider connection or audio device.

## Installed interface rendering

A fresh debug Debian package was built after the shared renderer readiness check was added. Its extracted executable passed the stricter smoke under Xvfb on NixOS: the packaged Svelte interface mounted, the real Bumblebee GLB loaded and Babylon completed a rendered frame before native, storage, asset and WebSocket checks completed. A subsequent check verified nontransparent model pixels and captured the complete native dashboard with the original animated bee. Xvfb requires software rendering and disabled DMABUF; those settings are confined to the isolated Linux CI helper and are not imposed on normal application startup. This establishes actual native webview rendering in the tested Nix environment. CI applies the installed check to the Windows installer, Linux Debian package and Linux AppImage.

## OBS Browser Source on NixOS

An isolated OBS Studio 32.1.2 instance rendered the unchanged production overlay assets through Browser Source at 1280×720 using the workstation's native NVIDIA graphics stack. An authenticated OBS WebSocket connection controlled only a temporary profile and scene. That scene contained only the browser source; browser audio was routed into OBS with monitoring disabled. No stream, microphone capture or desktop-audio capture was started. Original OBS configuration files were checksum-identical afterward.

The recorded 22.4-second H.264/AAC output contained the animated Bumblebee and word-highlighted captions, with 48 kHz stereo audio. The local fixture used previously generated speech and its word timings, requiring no new provider request. Four speech events fetched audio through the test server. The captured audio had nonzero speech (approximately −24 dBFS RMS) and exact zero PCM during the checked silence windows after both `stop_speech` and WebSocket disconnection. Across 553 OBS volume-meter events, residual audio ended within approximately 162 ms of cancellation/disconnection. The overlay reconnected approximately one second after disconnection and played a subsequent greeting. OBS reported streaming inactive with zero transmitted bytes.

This check used the real compiled renderer and an isolated HTTP/WebSocket fixture server, not the running Tauri application's transport or a live provider/Discord session. Those integration boundaries remain separate. It exposed a speech-bubble viewport-clipping issue at 1280×720. The renderer now constrains bubbles to horizontal and vertical viewport gutters, preserves original punctuation, and scrolls long captions within the bubble to follow the highlighted word. Seven focused regression tests cover captions up to 6,000 characters, small viewports, resizing and backward seeks; Svelte checking and the production build pass. The corrected overlay has not been visually rerun in OBS yet. Under Xvfb, the installed OBS browser's CEF 127 GPU process crashed with both default software graphics and explicit software ANGLE; native graphics succeeded. OBS also exited with a segmentation-fault status when the temporary instance was terminated after its recording finalized. These OBS environment/shutdown limitations are not treated as successful headless acceptance.
