import { describe, expect, test } from "bun:test";
import { parseToolPresentationPayload } from "./toolPresentation";

describe("parseToolPresentationPayload", () => {
	test("drops unsafe web document URLs and bounds source-like lists", () => {
		const payload = parseToolPresentationPayload({
			type: "tool.presentation.completed",
			presentationId: "presentation-1",
			tool: "researchWeb",
			completedAtMs: 123,
			content: {
				kind: "web_document",
				title: "Web result",
				summary: "Summary",
				body: "x".repeat(13_000),
				points: Array.from({ length: 20 }, (_, index) => `point ${index}`),
				sources: [
					{ title: "Bad", url: "javascript:alert(1)" },
					...Array.from({ length: 12 }, (_, index) => ({
						title: `Source ${index}`,
						url: `https://example.com/${index}`
					}))
				],
				previews: [
					{
						title: "Unsafe preview",
						url: "data:text/html,hi",
						imageUrl: "https://example.com/unused.png"
					},
					...Array.from({ length: 8 }, (_, index) => ({
						title: `Preview ${index}`,
						url: `https://preview.example/${index}`,
						imageUrl: index === 0 ? "javascript:alert(1)" : `https://images.example/${index}.png`
					}))
				],
				generatedAtMs: 123
			}
		});

		expect(payload?.type).toBe("tool.presentation.completed");
		if (
			payload?.type !== "tool.presentation.completed" ||
			payload.content.kind !== "web_document"
		) {
			throw new Error("Expected web document payload.");
		}
		expect(payload.content.body?.length).toBe(12_000);
		expect(payload.content.points).toHaveLength(8);
		expect(payload.content.sources).toHaveLength(8);
		expect(payload.content.sources[0]?.url).toBe("https://example.com/0");
		expect(payload.content.previews).toHaveLength(4);
		expect(payload.content.previews?.[0]?.url).toBe("https://preview.example/0");
		expect(payload.content.previews?.[0]?.imageUrl).toBeNull();
	});

	test("omits unsafe optional progress URLs", () => {
		const payload = parseToolPresentationPayload({
			type: "tool.presentation.progress",
			presentationId: "presentation-1",
			tool: "researchWeb",
			entry: {
				kind: "open_page",
				label: "Opening page",
				url: "file:///etc/passwd",
				imageUrl: "https://example.com/image.png"
			}
		});

		expect(payload?.type).toBe("tool.presentation.progress");
		if (payload?.type !== "tool.presentation.progress") {
			throw new Error("Expected progress payload.");
		}
		expect(payload.entry.url).toBeNull();
		expect(payload.entry.imageUrl).toBe("https://example.com/image.png");
	});
});
