import { describe, expect, test } from "bun:test";
import {
	clampOverlayRectAnchorPosition,
	normalizeOverlayRectAnchor,
	overlayRectAnchorFromPointPercent,
	overlayRectAnchorFromRect,
	overlayRectFromAnchorPosition,
	overlayRectPositionFromRect
} from "./overlayRectAnchor";

describe("overlay rect anchors", () => {
	test("normalizes invalid anchors to fallback", () => {
		expect(normalizeOverlayRectAnchor("top-right", "center")).toBe("top-right");
		expect(normalizeOverlayRectAnchor("nope", "bottom-left")).toBe("bottom-left");
		expect(normalizeOverlayRectAnchor(null)).toBe("center");
	});

	test("selects center anchor inside the configured center region", () => {
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 50, verticalPercent: 50 })).toBe(
			"center"
		);
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 25, verticalPercent: 75 })).toBe(
			"center"
		);
	});

	test("selects quadrant anchors outside the center region", () => {
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 10, verticalPercent: 10 })).toBe(
			"top-left"
		);
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 90, verticalPercent: 10 })).toBe(
			"top-right"
		);
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 10, verticalPercent: 90 })).toBe(
			"bottom-left"
		);
		expect(overlayRectAnchorFromPointPercent({ horizontalPercent: 90, verticalPercent: 90 })).toBe(
			"bottom-right"
		);
	});

	test("round-trips top-right anchor positions through rect geometry", () => {
		const viewport = { width: 1000, height: 500 };
		const rect = overlayRectFromAnchorPosition(
			{ horizontalPercent: 95, verticalPercent: 5 },
			"top-right",
			{ width: 100, height: 80 },
			viewport
		);
		expect(rect.left).toBe(850);
		expect(rect.top).toBe(25);
		expect(overlayRectPositionFromRect(rect, "top-right", viewport)).toEqual({
			horizontalPercent: 95,
			verticalPercent: 5
		});
	});

	test("detects anchors from rect center", () => {
		const viewport = { width: 1000, height: 500 };
		const rect = overlayRectFromAnchorPosition(
			{ horizontalPercent: 95, verticalPercent: 5 },
			"top-right",
			{ width: 100, height: 80 },
			viewport
		);
		expect(overlayRectAnchorFromRect(rect, viewport)).toBe("top-right");
	});

	test("clamps anchor points by anchor semantics", () => {
		expect(
			clampOverlayRectAnchorPosition({ horizontalPercent: 0, verticalPercent: 0 }, "top-right", {
				widthPercent: 20,
				heightPercent: 10
			})
		).toEqual({ horizontalPercent: 20, verticalPercent: 0 });
		expect(
			clampOverlayRectAnchorPosition({ horizontalPercent: 100, verticalPercent: 100 }, "center", {
				widthPercent: 20,
				heightPercent: 10
			})
		).toEqual({ horizontalPercent: 90, verticalPercent: 95 });
	});
});
