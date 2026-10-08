import { describe, expect, test } from "bun:test";
import { sampleFrameBubble, seekElementAnimations, type FrameBubbleOptions } from "./frameRenderer";
const options: FrameBubbleOptions = {
	text: "Hello there",
	startMs: 1000,
	durationMs: 1000,
	pose: { anchor: { x: 10, y: 10 }, tailVector: { x: 0, y: 10 }, subjectRadius: 10 },
	timeline: {
		words: [
			{ offsetMs: 0, durationMs: 400, text: "Hello" },
			{ offsetMs: 500, durationMs: 400, text: "there" }
		]
	}
};
describe("frame bubbles", () => {
	test("backwards and repeated seeks restore text and visibility", () => {
		const first = sampleFrameBubble(options, 1550);
		expect(first.text).toBe("Hello");
		expect(sampleFrameBubble(options, 1999).text).toBe("Hello there");
		expect(sampleFrameBubble(options, 1550)).toEqual(first);
		expect(sampleFrameBubble(options, 500).visible).toBe(false);
		expect(sampleFrameBubble(options, 2600).visible).toBe(false);
		expect(sampleFrameBubble(options, 2400).visible).toBe(true);
		expect(sampleFrameBubble(options, 2100).phase).toBe("leaving");
	});
	test("seeks all CSS animations including pseudo-element themes", () => {
		const animations = [
			{ currentTime: 0, pause() {} },
			{ currentTime: 0, pause() {} }
		];
		const element = {
			getAnimations: (args: { subtree: boolean }) => {
				expect(args.subtree).toBe(true);
				return animations;
			}
		};
		seekElementAnimations(element as unknown as HTMLElement, 1500);
		expect(animations.map((a) => a.currentTime)).toEqual([1500, 1500]);
		seekElementAnimations(element as unknown as HTMLElement, 200);
		expect(animations[1].currentTime).toBe(200);
	});
});
