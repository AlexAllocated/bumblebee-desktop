import { describe, expect, test } from "bun:test";
import { resolvePuppetDepthLayout, shouldCreatePuppetArtFin } from "./loadModel";

describe("puppet depth layout", () => {
	test.each([
		["full", 32, 12],
		["half", 16, 6],
		["flat", 2, 1]
	] as const)("preserves the %s construction path", (mode, artPlanes, stickPlanes) => {
		const layout = resolvePuppetDepthLayout(mode);

		expect(layout.artZ).toHaveLength(artPlanes);
		expect(layout.stickZ).toHaveLength(stickPlanes);
		if (mode === "flat") expect(layout.stickZ[0]).toBe(0);
	});

	test("omits the silhouette ribbon only in flat mode", () => {
		expect(shouldCreatePuppetArtFin("flat")).toBe(false);
		expect(shouldCreatePuppetArtFin("half")).toBe(true);
		expect(shouldCreatePuppetArtFin("full")).toBe(true);
	});
});
