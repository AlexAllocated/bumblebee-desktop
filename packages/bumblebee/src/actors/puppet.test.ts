import { describe, expect, test } from "bun:test";
import { puppetPoseFromOptions } from "./puppet";

describe("puppetPoseFromOptions", () => {
	test("preserves scale and stick occlusion when a cached puppet is reconfigured", () => {
		expect(
			puppetPoseFromOptions({
				position: { x: 0.75, y: 1 },
				scale: 0.36,
				occlusion: 0.5
			})
		).toEqual({
			position: { horizontalPercent: 75 },
			scale: 0.36,
			occlusion: 0.5
		});
	});

	test("does not reset omitted pose fields during reuse", () => {
		expect(puppetPoseFromOptions({ visible: false, hideAfterSpeech: true })).toEqual({});
	});
});
