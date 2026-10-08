import { describe, expect, test } from "bun:test";
import { chatBubbleTextMetrics, layoutEllipseText } from "../src/textLayout";

describe("chat bubble text metrics", () => {
	test("uses a compact readable size across phone and desktop viewports", () => {
		expect(chatBubbleTextMetrics({ viewportWidthPx: 390 }).fontSizePx).toBe(16);
		expect(chatBubbleTextMetrics({ viewportWidthPx: 1440 }).fontSizePx).toBeCloseTo(20.88);
		expect(chatBubbleTextMetrics({ viewportWidthPx: 2560 }).fontSizePx).toBe(21.25);
	});

	test("uses the compact fallback size when no viewport is available", () => {
		expect(chatBubbleTextMetrics({}).fontSizePx).toBe(21);
	});

	test("centers complete measured rows inside ellipse bounds without phantom offsets", () => {
		const text = "one two three four five six seven eight nine";
		const lineHeight = 20;
		const layout = layoutEllipseText({
			text,
			maxWidth: 260,
			maxHeight: 180,
			minWidth: 180,
			lineHeight,
			scale: 1,
			measureText: (value) => value.length * 9
		});

		expect(layout.fits).toBe(true);
		expect(layout.lines.length).toBeGreaterThan(1);
		expect(layout.lines.map((line) => line.text).join(" ")).toBe(text);
		expect(layout.lines[0]?.top).toBeCloseTo(lineHeight * 0.92);
		for (const line of layout.lines) {
			expect(line.top + lineHeight).toBeLessThan(layout.height);
			expect(line.width).toBeLessThanOrEqual(line.availableWidth);
		}
	});
});
