import { describe, expect, test } from "bun:test";
import type { PuppetRenderMetadata } from "../types";
import {
	buildPuppetAlphaBounds,
	buildPuppetRenderMetadata,
	buildPuppetSilhouetteMeasurements,
	normalizePuppetRenderRibbonPath,
	renderMetadataContentKey
} from "./renderMetadata";

const makeTestPixels = () => {
	const width = 8;
	const height = 8;
	const data = new Uint8Array(width * height * 4);
	for (let y = 1; y <= 6; y++) {
		for (let x = 2; x <= 5; x++) {
			const index = 4 * (y * width + x);
			data[index] = 255;
			data[index + 1] = 128;
			data[index + 2] = 64;
			data[index + 3] = 255;
		}
	}
	return { width, height, data };
};

const makeDominantEdgePixels = () => {
	const width = 16;
	const height = 16;
	const data = new Uint8Array(width * height * 4);
	for (let y = 2; y <= 13; y++) {
		for (let x = 2; x <= 13; x++) {
			const index = 4 * (y * width + x);
			const isInterior = x >= 5 && x <= 10 && y >= 5 && y <= 10;
			data[index] = isInterior ? 20 : 230;
			data[index + 1] = isInterior ? 80 : 24;
			data[index + 2] = isInterior ? 240 : 24;
			data[index + 3] = 255;
		}
	}
	return { width, height, data };
};

const makeInkAndFillEdgePixels = () => {
	const width = 20;
	const height = 20;
	const data = new Uint8Array(width * height * 4);
	for (let y = 3; y <= 16; y++) {
		for (let x = 3; x <= 16; x++) {
			const index = 4 * (y * width + x);
			const isInkEdge = x === 3 || y === 3 || (x === 4 && y <= 10);
			data[index] = isInkEdge ? 5 : 230;
			data[index + 1] = isInkEdge ? 5 : 200;
			data[index + 2] = isInkEdge ? 5 : 150;
			data[index + 3] = 255;
		}
	}
	return { width, height, data };
};

const makeMultiIslandPixels = () => {
	const width = 32;
	const height = 20;
	const data = new Uint8Array(width * height * 4);
	const fillRect = (left: number, top: number, right: number, bottom: number) => {
		for (let y = top; y <= bottom; y++) {
			for (let x = left; x <= right; x++) {
				const index = 4 * (y * width + x);
				data[index] = 220;
				data[index + 1] = 150;
				data[index + 2] = 80;
				data[index + 3] = 255;
			}
		}
	};
	fillRect(3, 4, 10, 15);
	fillRect(21, 5, 28, 14);
	return { width, height, data };
};

const makeHardIslandWithSoftGlowPixels = () => {
	const width = 40;
	const height = 24;
	const data = new Uint8Array(width * height * 4);
	for (let y = 5; y <= 18; y++) {
		for (let x = 4; x <= 13; x++) {
			const index = 4 * (y * width + x);
			data[index] = 230;
			data[index + 1] = 120;
			data[index + 2] = 80;
			data[index + 3] = 255;
		}
	}
	const centerX = 29;
	const centerY = 12;
	const radius = 8;
	for (let y = centerY - radius; y <= centerY + radius; y++) {
		for (let x = centerX - radius; x <= centerX + radius; x++) {
			const distance = Math.hypot(x - centerX, y - centerY);
			if (distance > radius) continue;
			const alpha = Math.round(Math.max(0, 1 - distance / radius) * 220);
			if (alpha <= 0) continue;
			const index = 4 * (y * width + x);
			data[index] = 120;
			data[index + 1] = 80;
			data[index + 2] = 255;
			data[index + 3] = alpha;
		}
	}
	return { width, height, data };
};

describe("puppet render metadata", () => {
	test("measures flat puppet bounds without generating ribbon metadata", () => {
		const measurements = buildPuppetAlphaBounds(makeTestPixels(), { sampleResolution: 8 });

		expect(measurements).not.toBeNull();
		expect(measurements!.visibleArtBounds.left).toBeCloseTo(-0.4286, 3);
		expect(measurements!.visibleArtBounds.right).toBeCloseTo(0.4286, 3);
		expect("ribbons" in measurements!).toBe(false);
	});

	test("can build runtime silhouette measurements without ribbon metadata", () => {
		const pixels = makeTestPixels();
		const measurements = buildPuppetSilhouetteMeasurements(pixels, { sampleResolution: 8 });
		const fullMetadata = buildPuppetRenderMetadata(pixels, { sampleResolution: 8 });

		expect(measurements).not.toBeNull();
		expect(fullMetadata).not.toBeNull();
		expect(measurements?.visibleHeight).toBeCloseTo(fullMetadata!.silhouette.visibleHeight);
		expect(measurements?.visibleArtBounds).toEqual(fullMetadata!.silhouette.visibleArtBounds);
		expect("ribbonPath" in measurements!).toBe(false);
		expect(fullMetadata!.silhouette.ribbons[0].path.length).toBeGreaterThan(0);
	});

	test("uses the dominant contour-adjacent color for ribbon metadata", () => {
		const metadata = buildPuppetRenderMetadata(makeDominantEdgePixels(), { sampleResolution: 16 });

		expect(metadata).not.toBeNull();
		expect(metadata!.silhouette.ribbons[0].edgeColor.r).toBeGreaterThan(0.8);
		expect(metadata!.silhouette.ribbons[0].edgeColor.b).toBeLessThan(0.2);
	});

	test("prefers contour fill color over dark neutral linework", () => {
		const metadata = buildPuppetRenderMetadata(makeInkAndFillEdgePixels(), {
			sampleResolution: 20
		});

		expect(metadata).not.toBeNull();
		expect(metadata!.silhouette.ribbons[0].edgeColor.r).toBeGreaterThan(0.8);
		expect(metadata!.silhouette.ribbons[0].edgeColor.g).toBeGreaterThan(0.65);
		expect(metadata!.silhouette.ribbons[0].edgeColor.b).toBeGreaterThan(0.45);
	});

	test("keeps separate hard-edged visible islands as separate ribbons", () => {
		const metadata = buildPuppetRenderMetadata(makeMultiIslandPixels(), { sampleResolution: 32 });

		expect(metadata).not.toBeNull();
		expect(metadata!.silhouette.ribbons.length).toBe(2);
		expect(metadata!.silhouette.visibleArtBounds.width).toBeGreaterThan(1.4);
	});

	test("does not create ribbons for soft alpha glow islands", () => {
		const metadata = buildPuppetRenderMetadata(makeHardIslandWithSoftGlowPixels(), {
			sampleResolution: 40
		});

		expect(metadata).not.toBeNull();
		expect(metadata!.silhouette.ribbons.length).toBe(1);
	});

	test("content key changes when ribbon geometry or edge color changes", () => {
		const metadata: PuppetRenderMetadata = {
			version: 2,
			image: { width: 32, height: 32 },
			silhouette: {
				sampleResolution: 32,
				visibleHeight: 2,
				visibleArtBounds: {
					left: -1,
					right: 1,
					top: 2,
					bottom: 0,
					width: 2,
					height: 2
				},
				ribbons: [
					{
						edgeColor: { r: 1, g: 0, b: 0 },
						path: [
							[-1, -1],
							[1, -1],
							[1, 1]
						]
					}
				]
			},
			generatedAt: "ignored"
		};
		const shiftedPath: PuppetRenderMetadata = {
			...metadata,
			silhouette: {
				...metadata.silhouette,
				ribbons: [
					{
						...metadata.silhouette.ribbons[0],
						path: [
							[-1, -1],
							[0.8, -1],
							[1, 1]
						]
					}
				]
			}
		};
		const recolored: PuppetRenderMetadata = {
			...metadata,
			silhouette: {
				...metadata.silhouette,
				ribbons: [{ ...metadata.silhouette.ribbons[0], edgeColor: { r: 0, g: 0.5, b: 1 } }]
			}
		};

		expect(renderMetadataContentKey(shiftedPath)).not.toBe(renderMetadataContentKey(metadata));
		expect(renderMetadataContentKey(recolored)).not.toBe(renderMetadataContentKey(metadata));
	});

	test("normalizes duplicate ribbon points while preserving valid geometry", () => {
		expect(
			normalizePuppetRenderRibbonPath([
				[-1, -1],
				[-1, -1],
				[1, -1],
				[1, 1],
				[-1, -1]
			])
		).toEqual([
			[-1, -1],
			[1, -1],
			[1, 1]
		]);
	});

	test("rejects corrupt ribbon coordinates before they reach Babylon geometry", () => {
		expect(
			normalizePuppetRenderRibbonPath([
				[-1, -1],
				[Number.NaN, 0],
				[1, 1]
			])
		).toBeNull();
		expect(
			normalizePuppetRenderRibbonPath([
				[-1, -1],
				[40, 0],
				[1, 1]
			])
		).toBeNull();
		expect(
			normalizePuppetRenderRibbonPath([
				[0, 0],
				[0, 0.0001],
				[0, 0.0002]
			])
		).toBeNull();
	});
});
