import { BUBBLE_TRANSITION_MS, createBubbleRenderer, type BubbleRendererOptions } from "./renderer";
import { createChatDecorationRevealSteps, type ChatDecorationRevealLayout } from "./reveal";
import type { BubblePose, SpeechTimeline } from "./types";

/** Sample CSS animations, including theme pseudo-elements, without a running clock. */
export const seekElementAnimations = (element: HTMLElement, timeMs: number) => {
	for (const animation of element.getAnimations({ subtree: true })) {
		animation.pause();
		animation.currentTime = Math.max(0, timeMs);
	}
};

export type FrameBubbleOptions = BubbleRendererOptions & {
	startMs: number;
	durationMs: number;
	timeline?: SpeechTimeline | null;
	/** Uses the same pagination/reveal calculation as live speech bubbles. */
	revealLayout?: ChatDecorationRevealLayout;
	exitDurationMs?: number;
};

export const sampleFrameBubble = (options: FrameBubbleOptions, timeMs: number) => {
	if (!Number.isFinite(timeMs)) throw new Error("Bubble frame time must be finite.");
	const elapsed = timeMs - options.startMs;
	const exitMs = options.exitDurationMs ?? BUBBLE_TRANSITION_MS;
	const viewport = typeof options.viewport === "function" ? options.viewport() : options.viewport;
	let text = "";
	let pageStartMs = 0;
	let previousChunk = -1;
	let cursor = 0;
	for (const step of createChatDecorationRevealSteps(options.text, options.timeline, {
		maxWidthPx: options.maxWidthPx,
		viewportWidthPx: viewport?.width,
		maxHeightPx: options.maxHeightPx,
		scale: options.scale,
		shape: options.style?.shape,
		audioDurationMs: options.durationMs,
		...options.revealLayout
	})) {
		cursor += step.delayMs;
		if (cursor > elapsed) break;
		if (step.chunkIndex !== previousChunk) pageStartMs = cursor;
		previousChunk = step.chunkIndex;
		text = step.text;
	}
	const leaving = elapsed >= options.durationMs;
	return {
		text,
		visible: elapsed >= 0 && elapsed < options.durationMs + exitMs && !!text,
		phase: leaving ? "leaving" : "entering",
		animationTimeMs: leaving ? elapsed - options.durationMs : elapsed - pageStartMs
	};
};

/** An explicitly sought alternative to the live layer. No scheduled reveal/hide work. */
export const createFrameBubbleRenderer = (options: FrameBubbleOptions) => {
	if (
		!Number.isFinite(options.startMs) ||
		!Number.isFinite(options.durationMs) ||
		options.durationMs <= 0
	) {
		throw new Error("Frame bubble requires finite timing and a positive duration.");
	}
	const renderer = createBubbleRenderer({ ...options, text: "" });
	let disposed = false;
	renderer.element.style.visibility = "hidden";
	return {
		element: renderer.element,
		seek(timeMs: number, pose: BubblePose = options.pose) {
			if (disposed) throw new Error("Frame bubble has been disposed.");
			const frame = sampleFrameBubble(options, timeMs);
			renderer.update({ text: frame.text, pose });
			renderer.element.style.visibility = frame.visible ? "visible" : "hidden";
			renderer.element.classList.add("is-visible", frame.phase);
			seekElementAnimations(renderer.element, frame.animationTimeMs);
			return frame;
		},
		dispose() {
			if (!disposed) renderer.dispose();
			disposed = true;
		}
	};
};
