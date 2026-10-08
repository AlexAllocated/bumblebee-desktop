# @hivetech/speech-bubbles

Framework-free SVG speech bubbles for browser overlays, games, and chat surfaces.

```ts
import { createBubbleLayer, defaultBubbleStyle } from "@hivetech/speech-bubbles";
import "@hivetech/speech-bubbles/style.css";

const layer = createBubbleLayer();

layer.show({
	id: "hello",
	text: "Hello from a standalone speech bubble.",
	pose: {
		anchor: { x: 420, y: 240 },
		tailVector: { x: -52, y: 32 },
		subjectRadius: 64
	},
	style: defaultBubbleStyle,
	autoReveal: true
});
```

The package has no Bumblebee SaaS dependency. Styles are passed as raw data, so apps can use
the default style, included decoration themes, or their own tokens.

For offline timelines, `createFrameBubbleRenderer(options)` accepts the same styles and
placement pose, plus `startMs`, `durationMs`, and an optional word-timing `timeline`.
Call `seek(absoluteTimeMs, optionalPose)` to sample reveal, pagination, entrance/exit and
theme CSS animations (including pseudo-elements), then `dispose()` when finished.
It reuses the live renderer and reveal/layout calculations without starting their timers.
`sampleFrameBubble` exposes the pure text/visibility sampler for testing.

## Workspace checks

Run `bun run --cwd packages/speech-bubbles check`, `test`, or `build` from the repository root.
The package has no provider credentials, desktop framework, or application backend dependency.
`@hivetech/speech-bubbles/reveal` also exports `speechTimelineCursorAt` to map precise audio clock
positions onto visible text without confusing SSML character offsets with display offsets.
