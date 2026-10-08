import {
	chatBubbleTextMetrics,
	textFitsChatBubbleLayout,
	type ChatBubbleTextShape
} from "./textLayout";

export type ChatDecorationSpeechTimeline = {
	visemes?: Array<{ offsetMs: number; visemeId: number }>;
	words?: Array<{
		offsetMs: number;
		durationMs: number;
		text: string;
		textOffset?: number;
		wordLength?: number;
	}>;
};

export type ChatDecorationRevealStep = {
	delayMs: number;
	text: string;
	chunkText: string;
	chunkIndex: number;
	fullCursor: number;
};

export type ChatDecorationRevealEvent = {
	delayMs: number;
	text: string;
	chunkText: string;
	chunkIndex: number;
	fullCursor: number;
	kind: "clear" | "reveal";
};

export type ChatDecorationRevealLayout = {
	maxWidthPx?: number;
	maxHeightPx?: number;
	scale?: number;
	shape?: ChatBubbleTextShape;
	viewportWidthPx?: number;
	minWidthPx?: number;
	fontSizePx?: number;
	lineHeightPx?: number;
	audioDurationMs?: number;
};

const FALLBACK_TICK_MS = 90;
const TIMELINE_REVEAL_LAG_MS = 90;
const FIT_CHUNK_PAGE_LINGER_MS = 650;

const cleanTimelineWord = (value: string) =>
	value
		.normalize("NFKC")
		.replace(/[^\p{L}\p{N}'-]+/gu, "")
		.trim();

const normalizeTimelinePhrase = (value: string) =>
	value.normalize("NFKC").trim().replace(/\s+/gu, " ");

const consumeTrailingPunctuation = (text: string, index: number) => {
	let next = index;
	while (next < text.length && /[\s,.;:!?)\]}"]/u.test(text[next] ?? "")) {
		next += 1;
	}
	return next;
};

const nextDisplayToken = (text: string, cursor: number) => {
	const start = Math.min(text.length, Math.max(0, cursor));
	const match = text.slice(start).match(/^\s*\S+\s*/u)?.[0];
	if (!match) return null;
	return {
		text: match.trim(),
		end: start + match.length
	};
};

const resolveTimelineWordCursor = ({
	text,
	cursor,
	word
}: {
	text: string;
	cursor: number;
	word: NonNullable<ChatDecorationSpeechTimeline["words"]>[number];
}) => {
	const cleanWord = cleanTimelineWord(word.text);
	if (!cleanWord) return null;
	const displayToken = nextDisplayToken(text, cursor);
	if (!displayToken) return null;
	if (cleanTimelineWord(displayToken.text) === cleanWord) return displayToken.end;

	const phrase = normalizeTimelinePhrase(word.text);
	if (phrase) {
		const leadingWhitespace = text.slice(cursor).match(/^\s*/u)?.[0].length ?? 0;
		const phraseStart = cursor + leadingWhitespace;
		const candidate = text.slice(phraseStart, phraseStart + phrase.length);
		if (normalizeTimelinePhrase(candidate).toLocaleLowerCase() === phrase.toLocaleLowerCase()) {
			return consumeTrailingPunctuation(text, phraseStart + phrase.length);
		}
	}

	return null;
};

const decorateStep = (
	text: string,
	fullCursor: number,
	delayMs: number
): ChatDecorationRevealStep => {
	const localEnd = Math.max(0, Math.min(text.length, fullCursor));
	return {
		delayMs,
		text: text.slice(0, localEnd).trim(),
		chunkText: text,
		chunkIndex: 0,
		fullCursor: localEnd
	};
};

const appendDecoratedCursorSteps = ({
	text,
	fromCursor,
	toCursor,
	delayMs,
	steps
}: {
	text: string;
	fromCursor: number;
	toCursor: number;
	delayMs: number;
	steps: ChatDecorationRevealStep[];
}) => {
	const nextCursor = Math.min(text.length, Math.max(fromCursor, toCursor));
	if (nextCursor <= fromCursor) return;
	const finalStep = decorateStep(text, nextCursor, delayMs);
	if (finalStep.text) steps.push(finalStep);
};

const appendFallbackCursorSteps = ({
	text,
	fromCursor,
	firstDelayMs,
	steps
}: {
	text: string;
	fromCursor: number;
	firstDelayMs: number;
	steps: ChatDecorationRevealStep[];
}) => {
	const start = Math.min(text.length, Math.max(0, fromCursor));
	const tokens = Array.from(text.slice(start).matchAll(/\S+\s*/gu));
	let cursor = start;
	for (const [index, token] of tokens.entries()) {
		const nextCursor = start + (token.index ?? 0) + token[0].length;
		appendDecoratedCursorSteps({
			text,
			fromCursor: cursor,
			toCursor: nextCursor,
			delayMs: index === 0 ? firstDelayMs : FALLBACK_TICK_MS,
			steps
		});
		cursor = nextCursor;
	}
};

const fallbackSteps = (
	text: string,
	layout?: ChatDecorationRevealLayout
): ChatDecorationRevealStep[] => {
	void layout;
	const steps: ChatDecorationRevealStep[] = [];
	appendFallbackCursorSteps({ text, fromCursor: 0, firstDelayMs: 0, steps });
	return steps.length
		? steps
		: [{ delayMs: 0, text, chunkText: text, chunkIndex: 0, fullCursor: text.length }];
};

type FitChunk = {
	start: number;
	end: number;
};

type FitToken = FitChunk & {
	text: string;
};

type MapStepsToFitChunksOptions = {
	pageTransitionDelayMs?: number;
};

const revealLayoutMetrics = (layout?: ChatDecorationRevealLayout) =>
	typeof layout?.fontSizePx === "number" && typeof layout.lineHeightPx === "number"
		? { fontSizePx: layout.fontSizePx, lineHeightPx: layout.lineHeightPx }
		: chatBubbleTextMetrics({
				scale: layout?.scale ?? 1,
				viewportWidthPx: layout?.viewportWidthPx
			});

const textFitsLayout = (text: string, layout?: ChatDecorationRevealLayout) => {
	if (!layout?.maxWidthPx || !layout.maxHeightPx) return true;
	const metrics = revealLayoutMetrics(layout);
	return textFitsChatBubbleLayout({
		text,
		shape: layout.shape,
		maxWidthPx: layout.maxWidthPx,
		maxHeightPx: layout.maxHeightPx,
		minWidthPx: layout.minWidthPx,
		fontSizePx: metrics.fontSizePx,
		lineHeightPx: metrics.lineHeightPx,
		scale: Math.max(0.1, layout.scale ?? 1)
	});
};

const fitTokensForText = (text: string): FitToken[] =>
	Array.from(text.matchAll(/\S+\s*/gu)).map((token) => ({
		start: token.index ?? 0,
		end: (token.index ?? 0) + token[0].length,
		text: token[0].trim()
	}));

const tokensInChunk = (tokens: FitToken[], chunk: FitChunk) =>
	tokens.filter((token) => token.start >= chunk.start && token.end <= chunk.end);

const chunkText = (text: string, chunk: FitChunk) => text.slice(chunk.start, chunk.end).trim();

const rebalanceSmallFitChunks = (
	text: string,
	chunks: FitChunk[],
	layout?: ChatDecorationRevealLayout
) => {
	if (chunks.length <= 1) return chunks;
	const tokens = fitTokensForText(text);
	const balanced = chunks.map((chunk) => ({ ...chunk }));
	const minimumWords = 3;

	for (let index = 0; index < balanced.length; index += 1) {
		let current = balanced[index];
		if (!current) continue;
		let currentTokens = tokensInChunk(tokens, current);
		if (currentTokens.length >= minimumWords) continue;

		const next = balanced[index + 1];
		if (next) {
			const nextTokens = tokensInChunk(tokens, next);
			const donor = nextTokens[0];
			if (donor && nextTokens.length > minimumWords) {
				const candidateCurrent = { start: current.start, end: donor.end };
				if (textFitsLayout(chunkText(text, candidateCurrent), layout)) {
					balanced[index] = candidateCurrent;
					balanced[index + 1] = { start: donor.end, end: next.end };
					current = candidateCurrent;
					currentTokens = tokensInChunk(tokens, current);
				}
			}
		}

		while (currentTokens.length < minimumWords) {
			const previous = balanced[index - 1];
			if (!previous) break;
			const previousTokens = tokensInChunk(tokens, previous);
			const donor = previousTokens.at(-1);
			if (!donor || previousTokens.length <= minimumWords) break;
			const candidateCurrent: FitChunk = { start: donor.start, end: current.end };
			if (!textFitsLayout(chunkText(text, candidateCurrent), layout)) break;
			balanced[index - 1] = { start: previous.start, end: donor.start };
			balanced[index] = candidateCurrent;
			current = candidateCurrent;
			currentTokens = tokensInChunk(tokens, current);
		}
	}

	return balanced;
};

const buildWordFitChunks = (text: string, layout?: ChatDecorationRevealLayout): FitChunk[] => {
	if (textFitsLayout(text, layout)) {
		return [{ start: 0, end: text.length }];
	}

	const chunks: FitChunk[] = [];
	const tokens = Array.from(text.matchAll(/\S+\s*/gu));
	let chunkStart = 0;
	let chunkEnd = 0;
	for (const token of tokens) {
		const tokenStart = token.index ?? chunkEnd;
		const tokenEnd = tokenStart + token[0].length;
		const nextText = text.slice(chunkStart, tokenEnd).trim();
		if (chunkEnd > chunkStart && !textFitsLayout(nextText, layout)) {
			chunks.push({ start: chunkStart, end: chunkEnd });
			chunkStart = tokenStart;
		}
		chunkEnd = tokenEnd;
		if (chunkEnd > chunkStart && !textFitsLayout(text.slice(chunkStart, chunkEnd).trim(), layout)) {
			chunks.push({ start: chunkStart, end: chunkEnd });
			chunkStart = chunkEnd;
		}
	}
	if (chunkEnd > chunkStart) chunks.push({ start: chunkStart, end: text.length });
	return chunks.length
		? rebalanceSmallFitChunks(text, chunks, layout)
		: [{ start: 0, end: text.length }];
};

const sentenceRangesForText = (text: string): FitChunk[] => {
	if (typeof Intl.Segmenter === "function") {
		const ranges = [...new Intl.Segmenter(undefined, { granularity: "sentence" }).segment(text)]
			.map((segment) => ({ start: segment.index, end: segment.index + segment.segment.length }))
			.filter((range) => text.slice(range.start, range.end).trim());
		if (ranges.length) return ranges;
	}

	const ranges: FitChunk[] = [];
	let start = 0;
	for (const boundary of text.matchAll(/[.!?]+(?=\s|$)/gu)) {
		const punctuationEnd = (boundary.index ?? 0) + boundary[0].length;
		const trailingWhitespace = text.slice(punctuationEnd).match(/^\s*/u)?.[0].length ?? 0;
		const end = punctuationEnd + trailingWhitespace;
		if (text.slice(start, end).trim()) ranges.push({ start, end });
		start = end;
	}
	if (text.slice(start).trim()) ranges.push({ start, end: text.length });
	return ranges.length ? ranges : [{ start: 0, end: text.length }];
};

const buildFitChunks = (text: string, layout?: ChatDecorationRevealLayout): FitChunk[] => {
	if (textFitsLayout(text, layout)) return [{ start: 0, end: text.length }];

	const sentences = sentenceRangesForText(text);
	if (sentences.length <= 1) return buildWordFitChunks(text, layout);

	const chunks: FitChunk[] = [];
	const appendSentenceGroup = (group: FitChunk) => {
		const groupText = text.slice(group.start, group.end);
		if (textFitsLayout(groupText.trim(), layout)) {
			chunks.push(group);
			return;
		}
		for (const chunk of buildWordFitChunks(groupText, layout)) {
			chunks.push({ start: group.start + chunk.start, end: group.start + chunk.end });
		}
	};

	let group = { ...sentences[0]! };
	for (const sentence of sentences.slice(1)) {
		const candidate = { start: group.start, end: sentence.end };
		if (textFitsLayout(text.slice(candidate.start, candidate.end).trim(), layout)) {
			group = candidate;
			continue;
		}
		appendSentenceGroup(group);
		group = { ...sentence };
	}
	appendSentenceGroup(group);
	return chunks;
};

const mapStepsToFitChunks = (
	text: string,
	steps: ChatDecorationRevealStep[],
	layout?: ChatDecorationRevealLayout,
	options?: MapStepsToFitChunksOptions
): ChatDecorationRevealStep[] => {
	const chunks = buildFitChunks(text, layout);
	if (chunks.length <= 1) return steps;

	const mapped: ChatDecorationRevealStep[] = [];
	const pageTransitionDelayMs = Math.max(
		0,
		options?.pageTransitionDelayMs ?? FIT_CHUNK_PAGE_LINGER_MS
	);
	let previousChunkIndex = 0;
	for (const step of steps) {
		const cursor = Math.min(text.length, Math.max(0, step.fullCursor));
		const chunkIndex = Math.max(
			0,
			chunks.findIndex((chunk) => cursor > chunk.start && cursor <= chunk.end)
		);
		const chunk = chunks[chunkIndex] ?? chunks[chunks.length - 1] ?? { start: 0, end: text.length };
		const localEnd = Math.min(cursor, chunk.end);
		const chunkText = text.slice(chunk.start, chunk.end).trim();
		const nextText = text.slice(chunk.start, localEnd).trim();
		if (!nextText) continue;
		const changedChunk = mapped.length > 0 && chunkIndex !== previousChunkIndex;
		let delayMs = step.delayMs + (changedChunk ? pageTransitionDelayMs : 0);
		if (changedChunk) {
			for (
				let boundaryChunkIndex = previousChunkIndex;
				boundaryChunkIndex < chunkIndex;
				boundaryChunkIndex += 1
			) {
				const boundaryChunk = chunks[boundaryChunkIndex];
				if (!boundaryChunk) continue;
				const boundaryText = text.slice(boundaryChunk.start, boundaryChunk.end).trim();
				const lastMapped = mapped.at(-1);
				if (
					!boundaryText ||
					(lastMapped?.chunkIndex === boundaryChunkIndex && lastMapped.text === boundaryText)
				) {
					continue;
				}
				mapped.push({
					delayMs: boundaryChunkIndex === previousChunkIndex ? step.delayMs : pageTransitionDelayMs,
					text: boundaryText,
					chunkText: boundaryText,
					chunkIndex: boundaryChunkIndex,
					fullCursor: boundaryChunk.end
				});
				delayMs = pageTransitionDelayMs;
			}
		}
		mapped.push({
			delayMs,
			text: nextText,
			chunkText,
			chunkIndex,
			fullCursor: cursor
		});
		previousChunkIndex = chunkIndex;
	}
	return mapped.length ? mapped : steps;
};

const paceStepsToAudioDuration = (
	steps: ChatDecorationRevealStep[],
	layout?: ChatDecorationRevealLayout
) => {
	const audioDurationMs = layout?.audioDurationMs;
	if (!audioDurationMs || !Number.isFinite(audioDurationMs) || audioDurationMs <= 0) return steps;
	if (steps.length <= 1) return steps;

	const totalRevealMs = steps.reduce((total, step) => total + Math.max(0, step.delayMs), 0);
	const targetFinalRevealMs = Math.max(0, audioDurationMs - Math.max(900, audioDurationMs * 0.1));
	if (totalRevealMs >= targetFinalRevealMs - 250) return steps;

	const absoluteTimes: number[] = [];
	let elapsedMs = 0;
	for (const step of steps) {
		elapsedMs += Math.max(0, step.delayMs);
		absoluteTimes.push(elapsedMs);
	}

	const firstRevealMs = absoluteTimes[0] ?? 0;
	const stretchableMs = totalRevealMs - firstRevealMs;
	if (stretchableMs <= 0) return steps;

	const factor = (targetFinalRevealMs - firstRevealMs) / stretchableMs;
	if (!Number.isFinite(factor) || factor <= 1) return steps;

	let previousMs = 0;
	return steps.map((step, index) => {
		const currentMs = absoluteTimes[index] ?? previousMs;
		const pacedMs = index === 0 ? currentMs : firstRevealMs + (currentMs - firstRevealMs) * factor;
		const delayMs = Math.max(0, pacedMs - previousMs);
		previousMs = pacedMs;
		return { ...step, delayMs };
	});
};

const createChatDecorationRevealSteps = (
	text: string,
	timeline: ChatDecorationSpeechTimeline | null | undefined,
	layout?: ChatDecorationRevealLayout
): ChatDecorationRevealStep[] => {
	const words = timeline?.words?.filter((word) => word.text.trim());
	if (!text || !words?.length)
		return paceStepsToAudioDuration(
			mapStepsToFitChunks(text, fallbackSteps(text, layout), layout),
			layout
		);

	const steps: ChatDecorationRevealStep[] = [];
	let cursor = 0;
	let previousRevealMs = 0;
	let awaitingTimelineResync = false;

	for (const word of words) {
		let nextCursor =
			resolveTimelineWordCursor({
				text,
				cursor,
				word
			}) ?? null;
		if (nextCursor === null) {
			if (!cleanTimelineWord(word.text)) continue;
			if (awaitingTimelineResync) continue;
			const nextToken = nextDisplayToken(text, cursor);
			nextCursor = nextToken?.end ?? text.length;
			awaitingTimelineResync = true;
		} else {
			awaitingTimelineResync = false;
		}
		nextCursor = Math.min(text.length, Math.max(cursor, nextCursor));
		if (nextCursor <= cursor) continue;

		const revealMs = Math.max(
			0,
			word.offsetMs + Math.max(0, word.durationMs) + TIMELINE_REVEAL_LAG_MS
		);
		const delayMs =
			steps.length === 0
				? Math.max(0, Math.min(650, revealMs))
				: Math.max(35, Math.min(900, revealMs - previousRevealMs));
		appendDecoratedCursorSteps({
			text,
			fromCursor: cursor,
			toCursor: nextCursor,
			delayMs,
			steps
		});
		cursor = nextCursor;
		previousRevealMs = revealMs;
	}

	if (cursor < text.length) {
		appendFallbackCursorSteps({
			text,
			fromCursor: cursor,
			firstDelayMs: FALLBACK_TICK_MS,
			steps
		});
	}

	const revealSteps = steps.length ? steps : fallbackSteps(text, layout);
	return paceStepsToAudioDuration(
		mapStepsToFitChunks(text, revealSteps, layout, {
			pageTransitionDelayMs: steps.length ? 0 : FIT_CHUNK_PAGE_LINGER_MS
		}),
		layout
	);
};

const createChatDecorationRevealEvents = (
	text: string,
	timeline: ChatDecorationSpeechTimeline | null | undefined,
	layout?: ChatDecorationRevealLayout
): ChatDecorationRevealEvent[] => {
	const steps = createChatDecorationRevealSteps(text, timeline, layout);
	return steps.map((step) => ({
		delayMs: Math.max(0, step.delayMs),
		text: step.text,
		chunkText: step.chunkText,
		chunkIndex: step.chunkIndex,
		fullCursor: step.fullCursor,
		kind: "reveal"
	}));
};

export { createChatDecorationRevealEvents, createChatDecorationRevealSteps };
