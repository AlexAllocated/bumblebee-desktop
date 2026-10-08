const DEFAULT_HANDLE_COLOR = "#FFCC88";

const HANDLE_COLOR_MAP: Record<string, string> = {
	default: DEFAULT_HANDLE_COLOR,
	red: "#ff0000",
	orange: "#ff7f00",
	legendary_orange: "#ff8c1a",
	yellow: "#ffff00",
	green: "#00ff00",
	blue: "#0000ff",
	violet: "#8b00ff"
};

const normalizeHandleToken = (value?: string | null) =>
	value
		?.toLowerCase()
		.replace(/[^a-z0-9]+/g, "_")
		.replace(/^_+|_+$/g, "") ?? "";

const resolveHandleColor = (
	handleId?: string | null,
	handleName?: string | null
): string | null => {
	const tryResolve = (token?: string | null): string | null => {
		if (!token) return null;
		const key = normalizeHandleToken(token);
		if (!key) return null;
		return HANDLE_COLOR_MAP[key] ?? null;
	};

	const directId = tryResolve(handleId);
	if (directId) return directId;

	const aliasId = tryResolve(handleId?.replace(/[_-]+/g, " "));
	if (aliasId) return aliasId;

	if (handleName) {
		const cleaned = handleName.toLowerCase().replace(/[^a-z0-9\s]+/g, " ");
		const tokens = cleaned.split(/\s+/).filter(Boolean);
		for (const token of tokens) {
			const resolved = tryResolve(token);
			if (resolved) return resolved;
		}
		const collapsed = tryResolve(cleaned);
		if (collapsed) return collapsed;
	}

	return null;
};

export { DEFAULT_HANDLE_COLOR, resolveHandleColor };
