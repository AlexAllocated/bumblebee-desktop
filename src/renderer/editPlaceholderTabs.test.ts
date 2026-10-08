import { describe, expect, test } from "bun:test";
import {
	isPointInEditPlaceholderDragArea,
	resolveEditPlaceholderTabBounds,
	resolveEditPlaceholderTabSide
} from "./editPlaceholderTabs";

describe("edit placeholder tab placement", () => {
	test("keeps the title tab above the placeholder when there is room", () => {
		expect(
			resolveEditPlaceholderTabSide({ top: 80, bottom: 280, height: 200 }, 32, { height: 720 })
		).toBe("top");
	});

	test("moves the title tab below a placeholder near the top viewport edge", () => {
		const side = resolveEditPlaceholderTabSide({ top: 0, bottom: 180, height: 180 }, 32, {
			height: 720
		});

		expect(side).toBe("bottom");
		expect(
			resolveEditPlaceholderTabBounds({ left: 540, right: 720, top: 0, bottom: 180 }, 32, side)
		).toEqual({ left: 540, right: 720, top: 180, bottom: 212 });
	});

	test("keeps the title tab above a placeholder near the bottom viewport edge", () => {
		expect(
			resolveEditPlaceholderTabSide({ top: 500, bottom: 720, height: 220 }, 32, { height: 720 })
		).toBe("top");
	});

	test("falls back inside the placeholder when neither outside edge has room", () => {
		expect(
			resolveEditPlaceholderTabSide({ top: 0, bottom: 720, height: 720 }, 32, { height: 720 })
		).toBe("inside-top");

		expect(
			resolveEditPlaceholderTabSide({ top: 12, bottom: 720, height: 708 }, 32, { height: 720 })
		).toBe("inside-bottom");
	});

	test("uses the rendered tab side for the draggable area", () => {
		const rect = { left: 540, right: 720, top: 0, bottom: 180, height: 180 };

		expect(isPointInEditPlaceholderDragArea(620, 196, rect, 32, { height: 720 })).toBe(true);
		expect(isPointInEditPlaceholderDragArea(620, -16, rect, 32, { height: 720 })).toBe(false);
	});
});
