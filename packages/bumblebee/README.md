# @hivetech/bumblebee

Reusable browser runtime for Bumblebee and chat puppets, used by the desktop preview and OBS overlay.
It owns actors, animation, Web Audio playback, bounded speech sequencing, nameplates, and the
`@hivetech/speech-bubbles` renderer. Applications own provider access and durable state.

```ts
import { createOverlay } from "@hivetech/bumblebee";
import "@hivetech/bumblebee/style.css";
import "@hivetech/speech-bubbles/style.css";

const overlay = await createOverlay({
  surface: { container: document.querySelector("#stage")! },
  assetBaseUrl: new URL("./assets/", document.baseURI),
  assets: { bumblebeeModelUrl: "/assets/bumblebee.glb" },
});
const bee = await overlay.bumblebee();
await bee.fly();
await bee.say({
  text: "Hello there!",
  audio: {
    url: "/media/hello.wav",
    words: [
      { text: "Hello", startMs: 100, durationMs: 300 },
      { text: "there", startMs: 450, durationMs: 250 },
    ],
  },
});

const puppet = await overlay.puppet("dandy", {
  position: { x: 0.76, y: 0.82 }, scale: 0.5, visible: true,
});
await puppet.moveTo({ x: 0.6, y: 0.8 });
overlay.interruptSpeech("user canceled");
await overlay.dispose();
```

`assetBaseUrl` defaults to the current document directory. The asset resolver never falls back to a
hosted Bumblebee service. Applications supply packaged models, puppet images, and sounds, or override
`assets.puppetImageUrl`, `assets.bumblebeeModelUrl`, and `assets.soundEffectUrls`. Artwork has separate
permissions from the MIT code; see the repository's `ASSETS.md`.

Speech accepts prepared local audio and exact provider word times. The host may instead inject
`prepareSpeech(speech, signal)` into `createOverlay` and use `bee.say("Hello")`; that callback owns
synthesis and must honor cancellation. No credentials or provider clients belong in this package.
The package queues at most eight pending utterances, uses the AudioContext clock for caption reveal,
and clears active and queued speech on interruption. `speech:start` fires before the source starts
so applications can update volume and caption presentation. `visuals: false` keeps audio without
showing the actor; `bubbles` and `nameplates` independently control those decorations.

The retained actor API includes poses, movement, emotes, adoption of external actors, speech bubbles,
playback pause, transitions, and explicit disposal. Low-level rendering tools remain available from
`@hivetech/bumblebee/low-level`; puppet metadata tooling uses `@hivetech/bumblebee/tools`.

## Frame-driven rendering

Offline compositions opt into a separate public controller; the live overlay API is unchanged.
This entry point works from the built npm distribution and has no HyperFrames dependency.

```ts
import { createFrameRenderer } from "@hivetech/bumblebee/frame-renderer";
import { createBumblebeeTimelineBuilder } from "@hivetech/bumblebee/timeline";
import "@hivetech/bumblebee/style.css";
import "@hivetech/speech-bubbles/style.css";

const timeline = createBumblebeeTimelineBuilder({
	initialPose: { position: { x: 0.5, y: 0.6 }, scale: 0.4 }
})
	.stance("flying", 0)
	.say(preparedSpeech, { startMs: 1000 })
	.build();
const renderer = createFrameRenderer({
	container: document.querySelector("#stage")!, // positioned, fixed-size element
	width: 1080,
	height: 1920,
	durationMs: 30000,
	actors: [{ id: "bee", kind: "bumblebee", modelUrl: "/assets/bumblebee.glb", timeline }]
});
await renderer.ready;
renderer.seek(2500); // milliseconds; backwards and repeated seeks are supported
renderer.dispose();
```

The host prepares a `PreparedSpeechAsset` before rendering (text, audioSrc, audioDurationMs,
and a word/viseme timeline). `@hivetech/bumblebee/speech-assets` exposes an explicit injected
preparation helper for live audio; it never calls a provider itself. Speech actions
require audio URLs and word timings. The controller loads models/textures/fonts, but does
not fetch or play audio, connect a bot, or start an animation loop. The composition owns
audio readiness and soundtrack wiring. Missing models or unprepared speech fail explicitly.

Timeline positions use normalized canvas coordinates and the center of a stable, calibrated
stance envelope, rather than the live actor's feet. Scale is the fraction of the viewport
the envelope fits inside. The flying envelope retains the model's bobbing animation without
making bubble anchors chase animated bones. Puppet models use the existing flat construction
path and their real nameplates; theme/style overrides are accepted as raw package tokens.

`seek()` samples Babylon keyframes, speech motion, nameplates, SVG bubbles, CSS theme animations,
and word reveal from absolute time. The seed controls procedural wood-grain texture generation.
No wall-clock reveal/hide callbacks are scheduled. Use identical assets/fonts and browser/GPU
versions for reproducible export; browser text antialiasing may differ slightly across hosts.

## Workspace checks

From the repository root, run `bun run check:packages`, `bun run build:packages`, and `bun run test`.
Each package also supports `bun run --cwd packages/bumblebee test` (or `packages/speech-bubbles`).
Tests include geometry, frame seeking, bubble pagination, actor lifecycle, and prepared speech
cancellation/clock behavior. Built package exports are independent of the desktop application.

Actual WebGL, Web Audio, and OBS verification is a separate gate; consult `docs/verification.md`
for the current installed and browser evidence. Unit tests do not establish live audio output.
