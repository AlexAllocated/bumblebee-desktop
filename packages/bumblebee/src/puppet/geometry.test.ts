import { describe, expect, test } from "bun:test";
import { buildCenteredArtPlaneMeasurements, getPuppetNameplateLocalAnchor } from "./geometry";

describe("puppet geometry", () => {
	test("anchors nameplates directly below visible art, not the stick bottom", () => {
		const art = buildCenteredArtPlaneMeasurements(2, 2).visibleArtBounds;
		expect(getPuppetNameplateLocalAnchor(art)).toEqual({ x: 0, y: 1.93 });
		// Taller sticks move the art/anchor together; a smaller puppet scales the gap.
		expect(getPuppetNameplateLocalAnchor({ top: 7, bottom: 5 }).y).toBeCloseTo(4.93);
		expect(getPuppetNameplateLocalAnchor({ top: 3.5, bottom: 2.5 }).y).toBeCloseTo(2.465);
	});
	test("measures centered art planes from the stick-bottom puppet origin", () => {
		const measurements = buildCenteredArtPlaneMeasurements(2, 2);

		expect(measurements.visibleHeight).toBe(4);
		expect(measurements.visibleArtBounds).toEqual({
			left: -1,
			right: 1,
			top: 4,
			bottom: 2,
			width: 2,
			height: 2
		});
	});
});
