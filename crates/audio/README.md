# Native audio

This library embeds Bumblebee's Songbird voice runtime inside the desktop process.
A single Serenity gateway handles Discord chat, members, permissions and voice.
No local control server, Redis, service authentication or bot-account credentials
are embedded in the library.

`AudioRuntime::connect(token, NativeResources::from_bundle(resource_directory))`
returns the runtime and bounded audio/chat event receivers. The resource directory
contains `native/` and `keyword_models/`. Speech synthesis can initialize the same
`NativeResources` and run independently of Discord.

Voice events are typed Rust values. Audio frames are mono PCM16LE at 16 kHz;
`AudioEvent.sequence` advances even when the bounded queue drops a message. A
consumer must discard an in-progress utterance on a sequence gap. Wake detection
provides `verificationPcm`; the agent enables `stream_participant` to collect the
following utterance and resets listening on completion/cancellation. `open_listen`
provides a bounded follow-up window. `PresenceConfig::owner_only` is the safe
first-run policy; Settings can explicitly grant others permission to wake the bot.

The preserved Songbird fork in `vendor/songbird` retains the mixer output tick
used to align replay capture with the outgoing audio clock. Session tests cover
pre-roll ordering, source changes, permission revocation, native destruction
outside shared locks, playback completion, bounded queues and cancellation.
Dropping a playback future stops its own track. Interrupting invalidates already
queued playback so an old turn cannot resume after cancellation.

TEN VAD is retained with its original 10 ms/16 kHz windows, threshold, pre-roll,
wake models, permissions, and 1.2 second end-of-utterance silence policy. TEN VAD
is a separately licensed native dependency (Powered by ten-vad): its Apache-derived license includes
additional noncompete and distribution conditions. It is not MIT project code or
unmodified Apache-2.0. The exact upstream license is bundled with the application.
Actual voice sensitivity still needs live microphone/Discord testing.

## Native dependencies

Run `python3 scripts/prepare-native-audio.py` from the repository root before a
Tauri build. It verifies the pinned Microsoft Speech SDK 1.50.0 NuGet archive,
compiles the small C++ bridge, and assembles resources. The SDK and pretrained
keyword models are separate redistributable components under Microsoft's terms;
they are not MIT project code. The library uses SDK word-boundary events for TTS
timing, not estimated text timing. Synthesis runs on a blocking worker and checks
cancellation while polling the SDK. Opus compiles statically through Cargo. Pinned TEN VAD binaries are bundled
for Windows and Linux x64 with checksums and the exact upstream license. Runtime users do not need a compiler or installed SDK.

Linux needs the SDK's system dependencies (glibc, libstdc++, libuuid, OpenSSL and
ALSA). The AppImage must bundle non-system runtime dependencies; the Debian and
Nix definitions supply them. Windows uses the SDK's x64 DLLs and the C++ runtime.
`native-manifest.json` records SHA-256 digests of the prepared native resources
before Tauri bundling. Its `stage` and `resources` fields identify that provenance.
AppImage's linuxdeploy can subsequently rewrite ELF loader tags, including
`RPATH` to `RUNPATH`; the prepared digests are not final installed-file digests.
`SHA256SUMS.txt` verifies the actual downloadable installer bytes.

`cargo test -p bumblebee-audio` runs hardware-independent tests. After preparing
resources and the platform library path, explicitly run the ignored
`real_keyword_lifecycle_runs_on_one_worker` test to prove native loading and
scheduler responsiveness. This does not prove an audible Discord session,
Windows packaging, provider synthesis, OBS timing or microphone recognition.
