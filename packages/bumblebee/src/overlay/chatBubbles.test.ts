import { describe, expect, test } from "bun:test";
import { resolveBubbleDesignFitScale, resolveBubbleDisplayScale } from "./chatBubbles";

describe("design-viewport bubble scaling", () => {
	test("scales a 1080p bubble composition into a mobile 16:9 video", () => {
		const viewport = { left: 0, top: 0, width: 374, height: 210.375 };
		const designViewport = { width: 1920, height: 1080 };

		expect(resolveBubbleDesignFitScale({ viewport, designViewport })).toBeCloseTo(0.19479, 4);
		expect(resolveBubbleDisplayScale({ authoredScale: 0.5, viewport, designViewport })).toBeCloseTo(
			0.25871,
			4
		);
	});

	test("keeps the normal attached-bubble scale without a design viewport", () => {
		expect(
			resolveBubbleDisplayScale({
				authoredScale: 0.5,
				viewport: { width: 374, height: 210.375 }
			})
		).toBe(1);
	});
});
