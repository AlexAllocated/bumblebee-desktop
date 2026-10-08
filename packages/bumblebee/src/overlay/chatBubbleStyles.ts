import { defaultBubbleStyle } from "@hivetech/speech-bubbles";
import type { ChatBubbleStyleTokens } from "../types";
import { normalizeClassToken, normalizeCssColor, normalizeRequiredClassToken } from "./styleTokens";

const DEFAULT_CHAT_BUBBLE_STYLE: ChatBubbleStyleTokens = {
	background: defaultBubbleStyle.background,
	border: defaultBubbleStyle.border,
	text: defaultBubbleStyle.text,
	shadow: defaultBubbleStyle.shadow,
	tail: defaultBubbleStyle.tail ?? "center",
	fontFamily: defaultBubbleStyle.fontFamily ?? "comic",
	shape: defaultBubbleStyle.shape ?? "rectangle"
};

export const normalizeChatBubbleStyle = (
	style?: Partial<ChatBubbleStyleTokens> | null
): ChatBubbleStyleTokens | null => {
	if (!style || typeof style !== "object" || Array.isArray(style)) return null;
	return {
		background: normalizeCssColor(style.background, DEFAULT_CHAT_BUBBLE_STYLE.background),
		border: normalizeCssColor(style.border, DEFAULT_CHAT_BUBBLE_STYLE.border),
		text: normalizeCssColor(style.text, DEFAULT_CHAT_BUBBLE_STYLE.text),
		shadow: normalizeCssColor(style.shadow, DEFAULT_CHAT_BUBBLE_STYLE.shadow),
		placeholderColor:
			typeof style.placeholderColor === "string"
				? normalizeCssColor(style.placeholderColor, DEFAULT_CHAT_BUBBLE_STYLE.background)
				: undefined,
		tail: normalizeRequiredClassToken<ChatBubbleStyleTokens["tail"]>(
			style.tail,
			DEFAULT_CHAT_BUBBLE_STYLE.tail
		),
		fontFamily: normalizeRequiredClassToken<ChatBubbleStyleTokens["fontFamily"]>(
			style.fontFamily,
			DEFAULT_CHAT_BUBBLE_STYLE.fontFamily
		),
		shape: normalizeRequiredClassToken<ChatBubbleStyleTokens["shape"]>(
			style.shape,
			DEFAULT_CHAT_BUBBLE_STYLE.shape
		),
		decoration: normalizeClassToken<NonNullable<ChatBubbleStyleTokens["decoration"]>>(
			style.decoration
		),
		motion: normalizeClassToken<NonNullable<ChatBubbleStyleTokens["motion"]>>(style.motion)
	};
};
