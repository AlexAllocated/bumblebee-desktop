export type ToolPresentationTool = "generateImage" | "researchWeb";

export type ToolPresentationProgressEntry = {
	kind: "status" | "search" | "open_page" | "find_in_page" | "complete";
	label: string;
	url?: string | null;
	query?: string | null;
	title?: string | null;
	snippet?: string | null;
	imageUrl?: string | null;
	atMs?: number;
};

export type ToolPresentationImageContent = {
	kind: "image";
	mediaId: string;
	mimeType: "image/png" | "image/jpeg" | "image/webp" | "image/gif";
	width?: number;
	height?: number;
	alt?: string | null;
};

export type ToolPresentationWebDocumentSource = {
	title: string;
	url: string;
	displayUrl?: string | null;
};

export type ToolPresentationWebDocumentPreview = {
	title?: string | null;
	description?: string | null;
	imageUrl?: string | null;
	siteName?: string | null;
	url: string;
	displayUrl?: string | null;
};

export type ToolPresentationWebDocumentContent = {
	kind: "web_document";
	title: string;
	summary: string;
	body?: string | null;
	points: string[];
	sources: ToolPresentationWebDocumentSource[];
	previews?: ToolPresentationWebDocumentPreview[];
	generatedAtMs: number;
};

export type ToolPresentationContent =
	ToolPresentationImageContent | ToolPresentationWebDocumentContent;

export type ToolPresentationStartedPayload = {
	type: "tool.presentation.started";
	presentationId: string;
	tool: ToolPresentationTool;
	title?: string | null;
	requestText?: string | null;
	startedAtMs: number;
	displayMs?: number;
};

export type ToolPresentationProgressPayload = {
	type: "tool.presentation.progress";
	presentationId: string;
	tool: ToolPresentationTool;
	entry: ToolPresentationProgressEntry;
};

export type ToolPresentationCompletedPayload = {
	type: "tool.presentation.completed";
	presentationId: string;
	tool: ToolPresentationTool;
	title?: string | null;
	requestText?: string | null;
	completedAtMs: number;
	displayMs?: number;
	content: ToolPresentationContent;
};

export type ToolPresentationFailedPayload = {
	type: "tool.presentation.failed";
	presentationId: string;
	tool: ToolPresentationTool;
	message: string;
	failedAtMs: number;
};

export type ToolPresentationPayload =
	| ToolPresentationStartedPayload
	| ToolPresentationProgressPayload
	| ToolPresentationCompletedPayload
	| ToolPresentationFailedPayload;

const isRecord = (value: unknown): value is Record<string, unknown> =>
	Boolean(value && typeof value === "object" && !Array.isArray(value));

const MAX_SHORT_TEXT_LENGTH = 280;
const MAX_TITLE_LENGTH = 160;
const MAX_BODY_LENGTH = 12_000;
const MAX_PROGRESS_LABEL_LENGTH = 240;
const MAX_WEB_POINTS = 8;
const MAX_WEB_SOURCES = 8;
const MAX_WEB_PREVIEWS = 4;

const boundedText = (value: unknown, maxLength = MAX_SHORT_TEXT_LENGTH) => {
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	if (!trimmed) return null;
	return trimmed.length > maxLength ? trimmed.slice(0, maxLength).trimEnd() : trimmed;
};
const text = (value: unknown) => boundedText(value);
const optionalText = (value: unknown, maxLength = MAX_SHORT_TEXT_LENGTH) =>
	value === null || value === undefined ? null : boundedText(value, maxLength);
const httpUrl = (value: unknown) => {
	const raw = boundedText(value, 2048);
	if (!raw) return null;
	try {
		const url = new URL(raw);
		return url.protocol === "http:" || url.protocol === "https:" ? url.toString() : null;
	} catch {
		return null;
	}
};
const finiteNumber = (value: unknown) =>
	typeof value === "number" && Number.isFinite(value) ? value : null;
const positiveInteger = (value: unknown) => {
	const parsed = finiteNumber(value);
	return parsed !== null && Number.isInteger(parsed) && parsed > 0 ? parsed : null;
};
const tool = (value: unknown): ToolPresentationTool | null =>
	value === "generateImage" || value === "researchWeb" ? value : null;

const parseProgressEntry = (value: unknown): ToolPresentationProgressEntry | null => {
	if (!isRecord(value)) return null;
	const kind =
		value.kind === "status" ||
		value.kind === "search" ||
		value.kind === "open_page" ||
		value.kind === "find_in_page" ||
		value.kind === "complete"
			? value.kind
			: null;
	const label = boundedText(value.label, MAX_PROGRESS_LABEL_LENGTH);
	if (!kind || !label) return null;
	const atMs = finiteNumber(value.atMs);
	const title = optionalText(value.title, MAX_TITLE_LENGTH);
	const snippet = optionalText(value.snippet, MAX_SHORT_TEXT_LENGTH);
	const url = httpUrl(value.url);
	const imageUrl = httpUrl(value.imageUrl);
	return {
		kind,
		label,
		url,
		query: optionalText(value.query),
		...(title === null ? {} : { title }),
		...(snippet === null ? {} : { snippet }),
		...(imageUrl === null ? {} : { imageUrl }),
		...(atMs === null ? {} : { atMs })
	};
};

const parseContent = (value: unknown): ToolPresentationContent | null => {
	if (!isRecord(value)) return null;
	if (value.kind === "image") {
		const mediaId = text(value.mediaId);
		const mimeType =
			value.mimeType === "image/png" ||
			value.mimeType === "image/jpeg" ||
			value.mimeType === "image/webp" ||
			value.mimeType === "image/gif"
				? value.mimeType
				: null;
		if (!mediaId || !mimeType) return null;
		const width = positiveInteger(value.width);
		const height = positiveInteger(value.height);
		return {
			kind: "image",
			mediaId,
			mimeType,
			...(width === null ? {} : { width }),
			...(height === null ? {} : { height }),
			alt: optionalText(value.alt)
		};
	}
	if (value.kind === "web_document") {
		const title = boundedText(value.title, MAX_TITLE_LENGTH);
		const summary = boundedText(value.summary, MAX_SHORT_TEXT_LENGTH);
		const generatedAtMs = finiteNumber(value.generatedAtMs);
		if (!title || !summary || generatedAtMs === null) return null;
		const body = optionalText(value.body, MAX_BODY_LENGTH);
		const points = Array.isArray(value.points)
			? value.points
					.flatMap((point): string[] => {
						const parsed = boundedText(point, MAX_SHORT_TEXT_LENGTH);
						return parsed ? [parsed] : [];
					})
					.slice(0, MAX_WEB_POINTS)
			: [];
		const sources = Array.isArray(value.sources)
			? value.sources
					.flatMap((source): ToolPresentationWebDocumentSource[] => {
						if (!isRecord(source)) return [];
						const sourceTitle = boundedText(source.title, MAX_TITLE_LENGTH);
						const url = httpUrl(source.url);
						if (!sourceTitle || !url) return [];
						return [{ title: sourceTitle, url, displayUrl: optionalText(source.displayUrl) }];
					})
					.slice(0, MAX_WEB_SOURCES)
			: [];
		const previews = Array.isArray(value.previews)
			? value.previews
					.flatMap((preview): ToolPresentationWebDocumentPreview[] => {
						if (!isRecord(preview)) return [];
						const url = httpUrl(preview.url);
						if (!url) return [];
						return [
							{
								url,
								title: optionalText(preview.title, MAX_TITLE_LENGTH),
								description: optionalText(preview.description),
								imageUrl: httpUrl(preview.imageUrl),
								siteName: optionalText(preview.siteName),
								displayUrl: optionalText(preview.displayUrl)
							}
						];
					})
					.slice(0, MAX_WEB_PREVIEWS)
			: null;
		return {
			kind: "web_document",
			title,
			summary,
			body,
			points,
			sources,
			...(previews === null ? {} : { previews }),
			generatedAtMs
		};
	}
	return null;
};

export const parseToolPresentationPayload = (value: unknown): ToolPresentationPayload | null => {
	if (!isRecord(value)) return null;
	const type = text(value.type) ?? text(value.action);
	const presentationId = text(value.presentationId);
	const parsedTool = tool(value.tool);
	if (!type || !presentationId || !parsedTool) return null;
	if (type === "tool.presentation.started") {
		const startedAtMs = finiteNumber(value.startedAtMs);
		if (startedAtMs === null) return null;
		return {
			type,
			presentationId,
			tool: parsedTool,
			title: optionalText(value.title),
			requestText: optionalText(value.requestText),
			startedAtMs,
			displayMs: finiteNumber(value.displayMs) ?? undefined
		};
	}
	if (type === "tool.presentation.progress") {
		const entry = parseProgressEntry(value.entry);
		return entry ? { type, presentationId, tool: parsedTool, entry } : null;
	}
	if (type === "tool.presentation.completed") {
		const completedAtMs = finiteNumber(value.completedAtMs);
		const content = parseContent(value.content);
		if (completedAtMs === null || !content) return null;
		return {
			type,
			presentationId,
			tool: parsedTool,
			title: optionalText(value.title),
			requestText: optionalText(value.requestText),
			completedAtMs,
			displayMs: finiteNumber(value.displayMs) ?? undefined,
			content
		};
	}
	if (type === "tool.presentation.failed") {
		const message = text(value.message);
		const failedAtMs = finiteNumber(value.failedAtMs);
		return message && failedAtMs !== null
			? { type, presentationId, tool: parsedTool, message, failedAtMs }
			: null;
	}
	return null;
};
