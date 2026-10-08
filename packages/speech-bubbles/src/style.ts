import type { SpeechBubbleStyle } from "./types";

export const bubbleCornerRadiusForShape = (shape: SpeechBubbleStyle["shape"]) => {
	switch (shape) {
		case "rectangle":
			return 0;
		case "caption":
			return 7;
		case "terminal":
			return 10;
		case "burst":
			return 4;
		case "cloud":
			return 24;
		case "ribbon":
			return 14;
		case "rounded":
		default:
			return 26;
	}
};

export const defaultBubbleStyle: SpeechBubbleStyle = {
	background: "#fff7c8",
	border: "#2f2a22",
	text: "#211b16",
	shadow: "rgba(47, 42, 34, 0.25)",
	tail: "center",
	fontFamily: "comic",
	shape: "rounded"
};

export const resolveBubbleStyle = (
	style: SpeechBubbleStyle | null | undefined
): Required<Pick<SpeechBubbleStyle, "background" | "border" | "text" | "shadow">> &
	SpeechBubbleStyle => ({
	...defaultBubbleStyle,
	...(style ?? {})
});
