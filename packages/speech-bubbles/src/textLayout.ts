export type ChatBubbleTextShape =
	"rounded" | "cloud" | "caption" | "burst" | "terminal" | "ribbon" | "ellipse" | "rectangle";

export type EllipseTextLine = {
	text: string;
	left: number;
	top: number;
	width: number;
	availableWidth: number;
};

export type EllipseTextLayout = {
	width: number;
	height: number;
	lines: EllipseTextLine[];
	fits: boolean;
};

type EllipseLayoutOptions = {
	text: string;
	maxWidth: number;
	maxHeight?: number;
	minWidth: number;
	lineHeight: number;
	scale: number;
	measureText: (text: string) => number;
};

type FitLayoutOptions = {
	text: string;
	shape?: ChatBubbleTextShape;
	maxWidthPx?: number;
	maxHeightPx?: number;
	minWidthPx?: number;
	fontSizePx?: number;
	lineHeightPx?: number;
	scale?: number;
	measureText?: (text: string) => number;
};

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
const round = (value: number) => Math.round(value * 100) / 100;
const lineWidthSafety = 0.88;

export const DEFAULT_CHAT_BUBBLE_FONT_WEIGHT = 550;

const splitWords = (text: string) => text.trim().split(/\s+/).filter(Boolean);

const approximateTextWidth = (text: string, fontSizePx: number) => {
	let units = 0;
	for (const char of text) {
		if (char === " ") units += 0.36;
		else if (/[ilI.,'!:;]/u.test(char)) units += 0.4;
		else if (/[mwMW@#%&]/u.test(char)) units += 0.92;
		else if (/[A-Z]/u.test(char)) units += 0.76;
		else units += 0.64;
	}
	return units * fontSizePx;
};

export const chatBubbleTextMetrics = ({
	scale = 1,
	viewportWidthPx
}: {
	scale?: number;
	viewportWidthPx?: number;
}) => {
	const safeScale = Math.max(0.1, scale);
	const viewportFontPx =
		typeof viewportWidthPx === "number" && Number.isFinite(viewportWidthPx)
			? viewportWidthPx * 0.0145
			: 21;
	const baseFontSizePx = clamp(viewportFontPx, 16, 21.25);
	const fontSizePx = baseFontSizePx * safeScale;
	return {
		fontSizePx,
		lineHeightPx: fontSizePx * 1.16
	};
};

const availableWidthForLine = ({
	width,
	height,
	lineHeight,
	lineIndex,
	lineCount,
	scale
}: {
	width: number;
	height: number;
	lineHeight: number;
	lineIndex: number;
	lineCount: number;
	scale: number;
}) => {
	const verticalPadding = lineHeight * 0.86;
	const horizontalInset = Math.max(14, 18 * scale);
	if (lineCount <= 1) {
		return Math.max(24, width - Math.max(24, 30 * scale));
	}
	const rx = Math.max(1, width / 2 - 5 * scale);
	const ry = Math.max(1, height / 2 - 5 * scale);
	const lineY = verticalPadding + lineHeight * (lineIndex + 0.5);
	const normalizedY = clamp((lineY - height / 2) / ry, -1, 1);
	const chordWidth = 2 * rx * Math.sqrt(Math.max(0, 1 - normalizedY * normalizedY));
	const rowTaper =
		lineCount <= 2 ? 0 : Math.abs(lineIndex - (lineCount - 1) / 2) / ((lineCount - 1) / 2);
	const comfortInset = horizontalInset + rowTaper * 8 * scale;
	return Math.max(24, (chordWidth - comfortInset * 2) * lineWidthSafety);
};

const attemptEllipseLayout = ({
	words,
	wordWidths,
	spaceWidth,
	width,
	lineCount,
	lineHeight,
	scale,
	maxHeight
}: {
	words: string[];
	wordWidths: number[];
	spaceWidth: number;
	width: number;
	lineCount: number;
	lineHeight: number;
	scale: number;
	maxHeight?: number;
}): EllipseTextLayout | null => {
	const verticalPadding = lineHeight * 0.92;
	const height = Math.max(lineHeight * 2.35, lineHeight * lineCount + verticalPadding * 2);
	if (typeof maxHeight === "number" && height > maxHeight + 0.5) return null;

	const availableWidths = Array.from({ length: lineCount }, (_, lineIndex) =>
		availableWidthForLine({
			width,
			height,
			lineHeight,
			lineIndex,
			lineCount,
			scale
		})
	);
	type EllipseLinePartition = { start: number; end: number; width: number };
	const memo = new Map<string, EllipseLinePartition[] | null>();
	const partitionRows = (lineIndex: number, wordIndex: number): EllipseLinePartition[] | null => {
		if (lineIndex === lineCount) return wordIndex === words.length ? [] : null;
		const memoKey = `${lineIndex}:${wordIndex}`;
		if (memo.has(memoKey)) return memo.get(memoKey) ?? null;
		const remainingRows = lineCount - lineIndex;
		const remainingWords = words.length - wordIndex;
		if (remainingWords < remainingRows) {
			memo.set(memoKey, null);
			return null;
		}

		const candidates: EllipseLinePartition[] = [];
		let lineWidth = 0;
		for (let end = wordIndex; end < words.length; end += 1) {
			lineWidth += (wordWidths[end] ?? 0) + (end > wordIndex ? spaceWidth : 0);
			if (lineWidth > (availableWidths[lineIndex] ?? 0) + 0.5) break;
			const wordsAfter = words.length - (end + 1);
			if (wordsAfter < remainingRows - 1) break;
			candidates.push({ start: wordIndex, end: end + 1, width: lineWidth });
		}

		for (let index = candidates.length - 1; index >= 0; index -= 1) {
			const candidate = candidates[index];
			if (!candidate) continue;
			const rest = partitionRows(lineIndex + 1, candidate.end);
			if (!rest) continue;
			const result = [candidate, ...rest];
			memo.set(memoKey, result);
			return result;
		}

		memo.set(memoKey, null);
		return null;
	};

	const partitions = partitionRows(0, 0);
	if (!partitions) return null;
	const lines = partitions.map((partition, lineIndex) => {
		const availableWidth = availableWidths[lineIndex] ?? 0;
		return {
			text: words.slice(partition.start, partition.end).join(" "),
			width: round(partition.width),
			availableWidth: round(availableWidth),
			left: round((width - partition.width) / 2),
			top: round(verticalPadding + lineIndex * lineHeight)
		};
	});

	return {
		width: round(width),
		height: round(height),
		lines,
		fits: true
	};
};

export const layoutEllipseText = ({
	text,
	maxWidth,
	maxHeight,
	minWidth,
	lineHeight,
	scale,
	measureText
}: EllipseLayoutOptions): EllipseTextLayout => {
	const words = splitWords(text);
	const safeLineHeight = Math.max(14, lineHeight);
	const safeScale = Math.max(0.1, scale);
	const safeMinWidth = Math.max(1, minWidth);
	const safeMaxWidth = Math.max(safeMinWidth, maxWidth);
	const safeMaxHeight =
		typeof maxHeight === "number" && Number.isFinite(maxHeight)
			? Math.max(safeLineHeight * 2.35, maxHeight)
			: undefined;

	if (!words.length) {
		return {
			width: round(safeMinWidth),
			height: round(safeLineHeight * 2.35),
			lines: [],
			fits: true
		};
	}

	const wordWidths = words.map((word) => Math.max(1, measureText(word)));
	const spaceWidth = Math.max(4, measureText(" "));
	const totalWidth =
		wordWidths.reduce((sum, wordWidth) => sum + wordWidth, 0) +
		Math.max(0, words.length - 1) * spaceWidth;
	const longestWord = Math.max(...wordWidths);
	const idealWidth = clamp(
		Math.sqrt(totalWidth * safeLineHeight * 3.2),
		Math.max(safeMinWidth, longestWord + 56 * safeScale),
		safeMaxWidth
	);
	const widths = Array.from(
		new Set([
			round(idealWidth),
			round(Math.min(safeMaxWidth, idealWidth + 36 * safeScale)),
			round(Math.min(safeMaxWidth, idealWidth + 72 * safeScale)),
			round(safeMaxWidth)
		])
	).sort((a, b) => a - b);
	const maxLineCountByHeight = safeMaxHeight
		? Math.max(1, Math.floor((safeMaxHeight - safeLineHeight * 1.72) / safeLineHeight))
		: 8;
	const maxLineCount = Math.max(1, maxLineCountByHeight);
	let best: (EllipseTextLayout & { score: number }) | null = null;

	for (const width of widths) {
		for (let lineCount = 1; lineCount <= maxLineCount; lineCount += 1) {
			const layout = attemptEllipseLayout({
				words,
				wordWidths,
				spaceWidth,
				width,
				lineCount,
				lineHeight: safeLineHeight,
				scale: safeScale,
				maxHeight: safeMaxHeight
			});
			if (!layout) continue;
			const widestLine = Math.max(...layout.lines.map((line) => line.width), 0);
			const spareWidth = Math.max(0, width - widestLine);
			const score =
				layout.lines.length * 28 + layout.height * 0.035 + width * 0.045 + spareWidth * 0.015;
			if (!best || score < best.score) best = { ...layout, score };
		}
	}

	if (best) {
		const { score: _score, ...layout } = best;
		return layout;
	}

	return {
		width: round(safeMaxWidth),
		height: round(Math.min(safeMaxHeight ?? safeLineHeight * 2.35, safeLineHeight * 2.35)),
		lines: [
			{
				text: words.join(" "),
				width: round(Math.min(totalWidth, safeMaxWidth)),
				availableWidth: round(safeMaxWidth),
				left: round(Math.max(0, (safeMaxWidth - Math.min(totalWidth, safeMaxWidth)) / 2)),
				top: round(safeLineHeight * 0.86)
			}
		],
		fits: false
	};
};

export const textFitsChatBubbleLayout = ({
	text,
	shape = "rounded",
	maxWidthPx,
	maxHeightPx,
	minWidthPx = 180,
	fontSizePx,
	lineHeightPx,
	scale = 1,
	measureText
}: FitLayoutOptions) => {
	const cleanText = text.trim();
	if (!cleanText) return true;
	if (!maxWidthPx || !maxHeightPx) return true;

	const metrics =
		typeof fontSizePx === "number" && typeof lineHeightPx === "number"
			? { fontSizePx, lineHeightPx }
			: chatBubbleTextMetrics({ scale });
	const resolvedMeasureText =
		measureText ?? ((value: string) => approximateTextWidth(value, metrics.fontSizePx));

	if (shape === "ellipse") {
		return layoutEllipseText({
			text: cleanText,
			maxWidth: Math.max(1, maxWidthPx),
			maxHeight: Math.max(1, maxHeightPx),
			minWidth: Math.min(Math.max(1, minWidthPx), Math.max(1, maxWidthPx)),
			lineHeight: metrics.lineHeightPx,
			scale,
			measureText: resolvedMeasureText
		}).fits;
	}

	const horizontalPadding = Math.max(28, 48 * scale);
	const verticalPadding = Math.max(22, 40 * scale);
	const innerWidth = Math.max(24, maxWidthPx - horizontalPadding);
	const innerHeight = Math.max(metrics.lineHeightPx, maxHeightPx - verticalPadding);
	const lineCount = Math.max(1, Math.floor(innerHeight / metrics.lineHeightPx));
	const words = splitWords(cleanText);
	let usedLines = 1;
	let lineWidth = 0;
	const spaceWidth = resolvedMeasureText(" ");

	for (const word of words) {
		const wordWidth = resolvedMeasureText(word);
		const nextWidth = lineWidth > 0 ? lineWidth + spaceWidth + wordWidth : wordWidth;
		if (wordWidth > innerWidth) return false;
		if (nextWidth <= innerWidth) {
			lineWidth = nextWidth;
			continue;
		}
		usedLines += 1;
		if (usedLines > lineCount) return false;
		lineWidth = wordWidth;
	}

	return true;
};
