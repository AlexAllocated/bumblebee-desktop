import { describe, expect, test } from "bun:test";
import { applyBitcrusherToSamples } from "./bitcrusher";

describe("bitcrusher sample processing", () => {
	test("deterministically quantizes and holds samples", () => {
		const source = new Float32Array([0.01, 0.12, 0.23, 0.34, 0.45, 0.56]);
		const options = { bitDepth: 4, sampleRateHz: 8_000, mix: 1 } as const;
		const first = applyBitcrusherToSamples(source, 48_000, options);
		const second = applyBitcrusherToSamples(source, 48_000, options);

		expect(first).toEqual(second);
		expect(first).not.toEqual(source);
		expect(new Set(first).size).toBeLessThan(source.length);
	});

	test("preserves clean samples when the wet mix is zero", () => {
		const source = new Float32Array([-0.75, -0.1, 0.2, 0.9]);
		expect(applyBitcrusherToSamples(source, 48_000, { mix: 0 })).toEqual(source);
	});
});
