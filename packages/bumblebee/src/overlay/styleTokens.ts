const CLASS_TOKEN_PATTERN = /^[a-z][a-zA-Z0-9-]{0,63}$/u;
const HEX_COLOR_PATTERN = /^#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const UNSAFE_COLOR_PATTERN = /[;{}@]|url\s*\(|var\s*\(|expression\s*\(/i;

export const normalizeCssColor = (value: unknown, fallback: string) => {
	if (typeof value !== "string") return fallback;
	const trimmed = value.trim();
	if (!trimmed || trimmed.length > 96) return fallback;
	if (UNSAFE_COLOR_PATTERN.test(trimmed)) return fallback;
	if (typeof CSS !== "undefined" && typeof CSS.supports === "function") {
		return CSS.supports("color", trimmed) ? trimmed : fallback;
	}
	return HEX_COLOR_PATTERN.test(trimmed) ? trimmed : fallback;
};

export const normalizeClassToken = <T extends string>(
	value: unknown,
	fallback?: T
): T | undefined => {
	if (typeof value !== "string") return fallback;
	const trimmed = value.trim();
	if (CLASS_TOKEN_PATTERN.test(trimmed)) return trimmed as T;
	return fallback;
};

export const normalizeRequiredClassToken = <T extends string>(value: unknown, fallback: T): T =>
	normalizeClassToken(value, fallback) ?? fallback;
