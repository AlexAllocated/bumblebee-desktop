import { describe, expect, test } from "bun:test";

import { createRevealEvents } from "../src";
import { resolveBubbleLayoutBounds } from "../src/renderer";

describe("speech bubble reveal chunking", () => {
	const chunksForMobileEllipse = (text: string) => {
		const bounds = resolveBubbleLayoutBounds({
			viewport: { width: 390, height: 844 },
			maxWidthPercent: 35,
			maxHeightPercent: 25
		});
		const events = createRevealEvents(text, null, {
			maxWidthPx: bounds.maxWidthPx,
			maxHeightPx: bounds.maxHeightPx,
			minWidthPx: bounds.minWidthPx,
			scale: 1,
			shape: "ellipse",
			viewportWidthPx: bounds.viewport.width
		});
		return [...new Map(events.map((event) => [event.chunkIndex, event.chunkText])).values()];
	};

	test("uses renderer bounds when paging text on narrow mobile screens", () => {
		const bounds = resolveBubbleLayoutBounds({
			viewport: { width: 390, height: 844 },
			maxWidthPercent: 35,
			maxHeightPercent: 25
		});
		const text =
			"This is a pretty normal sentence that should not turn into a scattered pile of tiny two word bubbles on mobile screens.";

		const events = createRevealEvents(text, null, {
			maxWidthPx: bounds.maxWidthPx,
			maxHeightPx: bounds.maxHeightPx,
			minWidthPx: bounds.minWidthPx,
			scale: 1,
			shape: "ellipse",
			viewportWidthPx: bounds.viewport.width
		});
		const chunks = [
			...new Map(events.map((event) => [event.chunkIndex, event.chunkText])).values()
		];

		expect(bounds.maxWidthPx).toBe(370.5);
		expect(chunks.length).toBeLessThanOrEqual(4);
		expect(chunks.every((chunk) => chunk.trim().split(/\s+/u).length > 1)).toBe(true);
	});

	test("uses a fixed 95 percent width limit on portrait mobile screens", () => {
		const bounds = resolveBubbleLayoutBounds({
			viewport: { width: 320, height: 568 },
			maxWidthPercent: 15,
			minWidthPx: 400,
			maxWidthPx: 200
		});

		expect(bounds.maxWidthPx).toBe(304);
		expect(bounds.minWidthPx).toBe(304);
	});

	test("keeps configured bounds for a narrow embedded landscape viewport", () => {
		const bounds = resolveBubbleLayoutBounds({
			viewport: { width: 390, height: 219 },
			maxWidthPercent: 35,
			maxHeightPercent: 25
		});

		expect(bounds.maxWidthPx).toBeCloseTo(136.5);
		expect(bounds.maxHeightPx).toBeCloseTo(54.75);
		expect(bounds.minWidthPx).toBeCloseTo(136.5);
	});

	test("keeps configured width limits above the mobile breakpoint", () => {
		const bounds = resolveBubbleLayoutBounds({
			viewport: { width: 641, height: 900 },
			maxWidthPercent: 35,
			maxWidthPx: 760
		});

		expect(bounds.maxWidthPx).toBeCloseTo(224.35);
		expect(bounds.minWidthPx).toBe(180);
	});

	test("keeps hyphenated words from becoming orphan bubble pages", () => {
		const text =
			"The first release of chat puppet art is hand-drawn by me, Alopex! By purchasing chat puppets from our store you directly support me and help me bring more of my creations to life on your stream!";
		const chunks = chunksForMobileEllipse(text);

		expect(chunks).not.toContain("hand-drawn");
		expect(chunks.every((chunk) => chunk.trim().split(/\s+/u).length >= 3)).toBe(true);
	});

	test("uses complete sentences as page boundaries whenever each sentence fits", () => {
		const sentences = [
			"Alex builds reliable systems.",
			"He tests every change.",
			"The work stays maintainable."
		];
		const text = sentences.join(" ");
		const events = createRevealEvents(text, null, {
			maxWidthPx: 260,
			maxHeightPx: 105,
			minWidthPx: 180,
			scale: 1,
			shape: "rectangle",
			viewportWidthPx: 800
		});
		const chunks = [
			...new Map(events.map((event) => [event.chunkIndex, event.chunkText])).values()
		];

		expect(chunks.length).toBeGreaterThan(1);
		expect(chunks.every((chunk) => /[.!?]$/u.test(chunk.trim()))).toBe(true);
		for (const sentence of sentences) {
			expect(chunks.filter((chunk) => chunk.includes(sentence))).toHaveLength(1);
		}
	});

	test("does not treat common abbreviations as complete sentence pages", () => {
		const text =
			"Sign up for Mr. King's programming class. One class was enough to get Alex hooked.";
		const events = createRevealEvents(text, null, {
			maxWidthPx: 275,
			maxHeightPx: 105,
			minWidthPx: 180,
			scale: 1,
			shape: "rectangle",
			viewportWidthPx: 800
		});
		const chunks = [
			...new Map(events.map((event) => [event.chunkIndex, event.chunkText])).values()
		];

		expect(chunks).not.toContain("Sign up for Mr.");
		expect(chunks.some((chunk) => chunk.includes("Mr. King's programming class."))).toBe(true);
	});

	test("continues a partial speech timeline one whole word at a time", () => {
		const text =
			"Audible messages wait for a quiet gap in the Discord voice conversation instead of rudely interrupting and speaking over channel members. A non-intrusive waiting tone plays to let you know that when you're done talking Bumblebee has something to say. In addition, messages in the queue expire after a configurable duration so that TTS doesn't lag behind in busy stream chats. Those small manners make the whole room easier to listen to.";
		const timelineWords = "Audible messages wait for a quiet gap".split(" ");
		const events = createRevealEvents(
			text,
			{
				words: timelineWords.map((word, index) => ({
					text: word,
					offsetMs: index * 180,
					durationMs: 120
				}))
			},
			{
				maxWidthPx: 320,
				maxHeightPx: 160,
				minWidthPx: 180,
				scale: 1,
				shape: "rectangle",
				viewportWidthPx: 800,
				audioDurationMs: 18_000
			}
		);
		const gapEventIndex = events.findIndex((event) => event.text.endsWith("quiet gap"));
		const afterGap = events.slice(gapEventIndex + 1);

		expect(gapEventIndex).toBeGreaterThanOrEqual(0);
		expect(afterGap.length).toBeGreaterThan(10);
		expect(afterGap[0]?.text).toEndWith("quiet gap in");
		expect(afterGap[0]?.fullCursor).toBeLessThan(text.length);
		expect(events.at(-1)?.fullCursor).toBe(text.length);
	});

	test("does not mistake SSML-relative offsets for visible-text offsets", () => {
		const text =
			"Audible messages wait for a quiet gap in the Discord voice conversation instead of rudely interrupting and speaking over channel members. A non-intrusive waiting tone plays to let you know that when you're done talking Bumblebee has something to say. In addition, messages in the queue expire after a configurable duration so that TTS doesn't lag behind in busy stream chats. Those small manners make the whole room easier to listen to.";
		const events = createRevealEvents(
			text,
			{
				words: [
					{ offsetMs: 50, durationMs: 425, text: "Audible", textOffset: 273, wordLength: 7 },
					{ offsetMs: 500, durationMs: 475, text: "messages", textOffset: 281, wordLength: 8 },
					{ offsetMs: 1_000, durationMs: 200, text: "wait", textOffset: 290, wordLength: 4 },
					{ offsetMs: 1_225, durationMs: 150, text: "for", textOffset: 295, wordLength: 3 },
					{ offsetMs: 1_387.5, durationMs: 62.5, text: "a", textOffset: 299, wordLength: 1 },
					{ offsetMs: 1_475, durationMs: 325, text: "quiet", textOffset: 301, wordLength: 5 },
					{ offsetMs: 1_825, durationMs: 250, text: "gap", textOffset: 307, wordLength: 3 },
					{ offsetMs: 2_100, durationMs: 75, text: "in", textOffset: 311, wordLength: 2 },
					{ offsetMs: 2_187.5, durationMs: 112.5, text: "the", textOffset: 314, wordLength: 3 },
					{ offsetMs: 2_325, durationMs: 400, text: "Discord", textOffset: 318, wordLength: 7 }
				]
			},
			{ maxWidthPx: 320, maxHeightPx: 160, minWidthPx: 180, viewportWidthPx: 1_280 }
		);

		expect(events.slice(0, 10).map((event) => event.text)).toEqual([
			"Audible",
			"Audible messages",
			"Audible messages wait",
			"Audible messages wait for",
			"Audible messages wait for a",
			"Audible messages wait for a quiet",
			"Audible messages wait for a quiet gap",
			"Audible messages wait for a quiet gap in",
			"Audible messages wait for a quiet gap in the",
			"Audible messages wait for a quiet gap in the Discord"
		]);
		expect(events[1]?.fullCursor).toBeLessThan(50);
	});

	test("reveals one display token for a multi-word pronunciation expansion", () => {
		const text = "so that TTS doesn't lag behind";
		const events = createRevealEvents(text, {
			words: [
				{ offsetMs: 0, durationMs: 100, text: "so", textOffset: 273, wordLength: 2 },
				{ offsetMs: 150, durationMs: 100, text: "that", textOffset: 276, wordLength: 4 },
				{ offsetMs: 300, durationMs: 100, text: "text", textOffset: 281, wordLength: 4 },
				{ offsetMs: 450, durationMs: 100, text: "to", textOffset: 286, wordLength: 2 },
				{ offsetMs: 600, durationMs: 100, text: "speech", textOffset: 289, wordLength: 6 },
				{ offsetMs: 750, durationMs: 100, text: "doesn't", textOffset: -1, wordLength: 7 },
				{ offsetMs: 900, durationMs: 100, text: "lag", textOffset: 304, wordLength: 3 },
				{ offsetMs: 1_050, durationMs: 100, text: "behind", textOffset: 308, wordLength: 6 }
			]
		});

		expect(events.map((event) => event.text)).toEqual([
			"so",
			"so that",
			"so that TTS",
			"so that TTS doesn't",
			"so that TTS doesn't lag",
			"so that TTS doesn't lag behind"
		]);
	});
});
