import { describe, expect, test } from "bun:test";
import { defaultBubbleStyle } from "@hivetech/speech-bubbles";
import { normalizeChatBubbleStyle } from "./chatBubbleStyles";

describe("chat bubble style normalization", () => {
	test("uses the speech bubble default as the partial-style fallback", () => {
		const style = normalizeChatBubbleStyle({});

		expect(style).toMatchObject({
			background: defaultBubbleStyle.background,
			border: defaultBubbleStyle.border,
			text: defaultBubbleStyle.text,
			shadow: defaultBubbleStyle.shadow,
			tail: defaultBubbleStyle.tail,
			fontFamily: defaultBubbleStyle.fontFamily,
			shape: defaultBubbleStyle.shape
		});
	});

	test("keeps known style fields while dropping renderer custom properties", () => {
		const style = normalizeChatBubbleStyle({
			background: "#fff8cc",
			border: "url(https://example.com/bad)",
			text: "#111111",
			shadow: "#222222",
			tail: "center",
			fontFamily: "rounded",
			shape: "cloud",
			decoration: "sparkles",
			motion: "pulse",
			customProperties: { "--evil": "display:block" }
		} as never);

		expect(style).toEqual({
			background: "#fff8cc",
			border: "#2f2a22",
			text: "#111111",
			shadow: "#222222",
			placeholderColor: undefined,
			tail: "center",
			fontFamily: "rounded",
			shape: "cloud",
			decoration: "sparkles",
			motion: "pulse"
		});
		expect("customProperties" in (style as unknown as Record<string, unknown>)).toBe(false);
	});
});
