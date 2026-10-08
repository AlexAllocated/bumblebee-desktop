import { describe, expect, test } from "bun:test";
import { resolveFrameRateIndependentLerpFactor, shouldApplyAnchoredLerp } from "./bumblebee";

describe("Bumblebee anchor tracking", () => {
	test("tracks an element every render frame without requiring a scroll activity window", () => {
		expect(
			shouldApplyAnchoredLerp({
				anchorModeEnabled: true,
				hasAnchor: true,
				responsiveMoveActive: false
			})
		).toBe(true);
	});

	test("yields while a deliberate responsive move controls the same pose", () => {
		expect(
			shouldApplyAnchoredLerp({
				anchorModeEnabled: true,
				hasAnchor: true,
				responsiveMoveActive: true
			})
		).toBe(false);
	});

	test("preserves the authored interpolation at 60 FPS", () => {
		expect(resolveFrameRateIndependentLerpFactor(0.35, 1000 / 60)).toBeCloseTo(0.35, 6);
	});

	test("catches up proportionally when scrolling delays a render frame", () => {
		expect(resolveFrameRateIndependentLerpFactor(0.35, 130)).toBeGreaterThan(0.96);
	});
});
