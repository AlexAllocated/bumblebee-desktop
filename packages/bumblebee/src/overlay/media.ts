export type OverlayMediaPlayPayload = {
	type: "overlay.media.play";
	mediaId: string;
	kind: "video";
	url: string;
	contentType: "video/mp4";
	playbackId?: string;
	muted?: boolean;
	startPaused?: boolean;
	volume?: number;
	displayMs?: number;
	widthViewportPercent?: number;
	startedAtMs?: number;
};

export type OverlayMediaStartPayload = {
	type: "overlay.media.start";
	playbackId: string;
	startedAtMs?: number;
};

export type OverlayMediaStopPayload = {
	type: "overlay.media.stop";
	playbackId: string;
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
	Boolean(value && typeof value === "object" && !Array.isArray(value));

const trimmedString = (value: unknown) =>
	typeof value === "string" && value.trim() ? value.trim() : "";

const safeHttpUrl = (value: unknown) => {
	const raw = trimmedString(value);
	if (!raw) return "";
	try {
		const url = new URL(raw);
		return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : "";
	} catch {
		return "";
	}
};

const finiteNumberInRange = (value: unknown, min: number, max: number) =>
	typeof value === "number" && Number.isFinite(value)
		? Math.max(min, Math.min(max, value))
		: undefined;

export const parseOverlayMediaPlayPayload = (value: unknown): OverlayMediaPlayPayload | null => {
	if (!isRecord(value)) return null;
	if (value.type !== "overlay.media.play") return null;
	if (value.kind !== "video") return null;
	if (value.contentType !== "video/mp4") return null;
	const mediaId = trimmedString(value.mediaId);
	const url = safeHttpUrl(value.url);
	if (!mediaId || !url) return null;
	const payload: OverlayMediaPlayPayload = {
		type: "overlay.media.play",
		mediaId,
		kind: "video",
		url,
		contentType: "video/mp4"
	};
	const displayMs = finiteNumberInRange(value.displayMs, 1_000, 120_000);
	if (displayMs !== undefined) payload.displayMs = displayMs;
	const widthViewportPercent = finiteNumberInRange(value.widthViewportPercent, 5, 80);
	if (widthViewportPercent !== undefined) payload.widthViewportPercent = widthViewportPercent;
	const startedAtMs = finiteNumberInRange(value.startedAtMs, 0, Number.MAX_SAFE_INTEGER);
	if (startedAtMs !== undefined) payload.startedAtMs = startedAtMs;
	const playbackId = trimmedString(value.playbackId);
	if (playbackId) payload.playbackId = playbackId;
	if (typeof value.muted === "boolean") payload.muted = value.muted;
	if (typeof value.startPaused === "boolean") payload.startPaused = value.startPaused;
	const volume = finiteNumberInRange(value.volume, 0, 1);
	if (volume !== undefined) payload.volume = volume;
	return payload;
};

export const parseOverlayMediaStartPayload = (value: unknown): OverlayMediaStartPayload | null => {
	if (!isRecord(value)) return null;
	if (value.type !== "overlay.media.start") return null;
	const playbackId = trimmedString(value.playbackId);
	if (!playbackId) return null;
	const payload: OverlayMediaStartPayload = {
		type: "overlay.media.start",
		playbackId
	};
	const startedAtMs = finiteNumberInRange(value.startedAtMs, 0, Number.MAX_SAFE_INTEGER);
	if (startedAtMs !== undefined) payload.startedAtMs = startedAtMs;
	return payload;
};

export const parseOverlayMediaStopPayload = (value: unknown): OverlayMediaStopPayload | null => {
	if (!isRecord(value)) return null;
	if (value.type !== "overlay.media.stop") return null;
	const playbackId = trimmedString(value.playbackId);
	if (!playbackId) return null;
	return {
		type: "overlay.media.stop",
		playbackId
	};
};
