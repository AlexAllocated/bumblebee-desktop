import {
	DEFAULT_CHAT_BUBBLE_FONT_WEIGHT,
	chatBubbleTextMetrics,
	layoutEllipseText
} from "./textLayout";
import {
	clamp,
	normalizeVector,
	pointDistance,
	resolveTailBaseWidth,
	subjectCircleForPose,
	trackCircleForPose
} from "./geometry";
import { bubbleCornerRadiusForShape, resolveBubbleStyle } from "./style";
import { PausableScheduler, type PausableTimer } from "./pausableScheduler";
import type {
	BubbleDebug,
	BubbleBodyRect,
	BubblePose,
	BubbleUpdateOptions,
	Point,
	Rect,
	Size,
	SpeechBubbleShape,
	SpeechBubbleStyle,
	Viewport
} from "./types";

export type BubbleRendererOptions = {
	id?: string;
	text: string;
	pose: BubblePose;
	style?: SpeechBubbleStyle | null;
	target?: HTMLElement;
	scale?: number;
	maxWidthPercent?: number;
	maxHeightPercent?: number;
	minWidthPx?: number;
	maxWidthPx?: number;
	maxHeightPx?: number;
	fixedBodySize?: Size;
	fixedBodyRect?: BubbleBodyRect;
	zIndex?: number;
	placeholder?: boolean;
	debug?: boolean;
	constrainToViewport?: boolean;
	viewport?: Viewport | (() => Viewport);
	onDebug?: (debug: BubbleDebug) => void;
	scheduler?: PausableScheduler;
};

type BubbleRendererUpdateOptions = BubbleUpdateOptions & {
	debug?: boolean;
	placeholder?: boolean;
};

export type BubbleRendererHideOptions = {
	animate?: boolean;
};

export type BubbleRenderer = {
	readonly element: HTMLDivElement;
	readonly debug: BubbleDebug | null;
	update(options: BubbleRendererUpdateOptions): void;
	setPaused?(paused: boolean): void;
	show(): void;
	hide(options?: BubbleRendererHideOptions): Promise<void>;
	dispose(): void;
};

type Perimeter = {
	samples: Point[];
	cumulative: number[];
	length: number;
};

type TextLayout = {
	width: number;
	height: number;
	lines: string[];
	ellipseLines?: Array<{ text: string; left: number; top: number; width: number }>;
	fontSizePx: number;
	lineHeightPx: number;
	paddingX: number;
	paddingY: number;
	wrapped: boolean;
};

type MeasureLayoutParams = {
	text: string;
	style: SpeechBubbleStyle;
	scale: number;
	viewport: Required<Viewport>;
	maxWidthPercent: number;
	maxHeightPercent: number;
	minWidthPx: number;
	maxWidthPx: number;
	maxHeightPx?: number;
	fixedBodySize?: Size;
};

const SVG_NS = "http://www.w3.org/2000/svg";
export const BUBBLE_TRANSITION_MS = 520;
const DEFAULT_Z_INDEX = 2147483000;
export const DEFAULT_BUBBLE_MIN_WIDTH_PX = 180;
export const DEFAULT_BUBBLE_MAX_WIDTH_PX = 760;
const MOBILE_VIEWPORT_MAX_WIDTH_PX = 640;
const MOBILE_BUBBLE_MAX_WIDTH_PERCENT = 95;
const SHELL_PADDING = 9;

const point = (x: number, y: number): Point => ({
	x: Math.round(x * 100) / 100,
	y: Math.round(y * 100) / 100
});

const rect = (left: number, top: number, width: number, height: number): Rect => ({
	x: left,
	y: top,
	left,
	top,
	right: left + width,
	bottom: top + height,
	centerX: left + width / 2,
	centerY: top + height / 2,
	width,
	height
});

const normalizeViewport = (viewport?: Viewport | (() => Viewport)): Required<Viewport> => {
	const value = typeof viewport === "function" ? viewport() : viewport;
	return {
		left: value?.left ?? 0,
		top: value?.top ?? 0,
		width:
			typeof value?.width === "number" && Number.isFinite(value.width)
				? Math.max(1, value.width)
				: typeof window !== "undefined"
					? window.innerWidth
					: 1280,
		height:
			typeof value?.height === "number" && Number.isFinite(value.height)
				? Math.max(1, value.height)
				: typeof window !== "undefined"
					? window.innerHeight
					: 720
	};
};

const fontFamilyForStyle = (style: SpeechBubbleStyle) => {
	switch (style.fontFamily) {
		case "rounded":
			return "Inter, ui-rounded, system-ui, sans-serif";
		case "mono":
			return '"JetBrains Mono", "SFMono-Regular", Consolas, ui-monospace, monospace';
		case "serif":
			return 'Georgia, "Times New Roman", serif';
		default:
			return '"Trebuchet MS", "Comic Sans MS", ui-rounded, system-ui, sans-serif';
	}
};

let measureCanvasContext: CanvasRenderingContext2D | null = null;

const measureTextWidth = (text: string, font: string) => {
	if (typeof document !== "undefined") {
		measureCanvasContext ??= document.createElement("canvas").getContext("2d");
	}
	if (measureCanvasContext) {
		measureCanvasContext.font = font;
		return measureCanvasContext.measureText(text).width;
	}
	return text.length * 13;
};

const splitWords = (text: string) => text.trim().split(/\s+/u).filter(Boolean);

const wrapText = (text: string, maxWidth: number, measure: (value: string) => number) => {
	const words = splitWords(text);
	if (!words.length) return [""];
	const lines: string[] = [];
	let current = "";
	for (const word of words) {
		const candidate = current ? `${current} ${word}` : word;
		if (current && measure(candidate) > maxWidth) {
			lines.push(current);
			current = word;
		} else {
			current = candidate;
		}
	}
	if (current) lines.push(current);
	return lines;
};

const visibleTextForLayoutLines = (text: string, layoutLines: string[]) => {
	const visibleWords = splitWords(text);
	let wordIndex = 0;
	return layoutLines.map((line, lineIndex) => {
		const plannedWordCount = splitWords(line).length;
		const isLastLine = lineIndex === layoutLines.length - 1;
		const endIndex = isLastLine
			? visibleWords.length
			: Math.min(visibleWords.length, wordIndex + plannedWordCount);
		const visibleLine = visibleWords.slice(wordIndex, endIndex).join(" ");
		wordIndex = endIndex;
		return visibleLine;
	});
};

const shapePerimeter = (width: number, height: number, shape: SpeechBubbleShape): Perimeter => {
	const samples: Point[] = [];
	const add = (x: number, y: number) => {
		const next = point(x, y);
		const previous = samples.at(-1);
		if (previous && Math.abs(previous.x - next.x) < 0.01 && Math.abs(previous.y - next.y) < 0.01) {
			return;
		}
		samples.push(next);
	};
	const addPoints = (points: Point[]) => {
		for (const entry of points) add(entry.x, entry.y);
	};
	const decorativeOutset =
		shape === "cloud" || shape === "burst"
			? Math.min(width, height) * 0.2
			: shape === "ribbon"
				? Math.min(width, height) * 0.18
				: 0;
	const shapeLeft = -decorativeOutset;
	const shapeTop = -decorativeOutset;
	const shapeWidth = width + decorativeOutset * 2;
	const shapeHeight = height + decorativeOutset * 2;
	const addRoundedRect = (radius: number) => {
		const resolvedRadius = Math.min(radius, width / 2, height / 2);
		const arcSteps = resolvedRadius === 0 ? 1 : 14;
		const addArc = (cx: number, cy: number, start: number, end: number) => {
			for (let index = 1; index <= arcSteps; index += 1) {
				const t = index / arcSteps;
				const angle = start + (end - start) * t;
				add(cx + Math.cos(angle) * resolvedRadius, cy + Math.sin(angle) * resolvedRadius);
			}
		};
		add(resolvedRadius, 0);
		add(width - resolvedRadius, 0);
		addArc(width - resolvedRadius, resolvedRadius, -Math.PI / 2, 0);
		add(width, height - resolvedRadius);
		addArc(width - resolvedRadius, height - resolvedRadius, 0, Math.PI / 2);
		add(resolvedRadius, height);
		addArc(resolvedRadius, height - resolvedRadius, Math.PI / 2, Math.PI);
		add(0, resolvedRadius);
		addArc(resolvedRadius, resolvedRadius, Math.PI, (Math.PI * 3) / 2);
	};

	if (shape === "ellipse") {
		const steps = 96;
		const rx = shapeWidth / 2;
		const ry = shapeHeight / 2;
		const cx = shapeLeft + shapeWidth / 2;
		const cy = shapeTop + shapeHeight / 2;
		for (let index = 0; index < steps; index += 1) {
			const angle = -Math.PI / 2 + (Math.PI * 2 * index) / steps;
			add(cx + Math.cos(angle) * rx, cy + Math.sin(angle) * ry);
		}
	} else if (shape === "cloud") {
		const steps = 104;
		const rx = shapeWidth / 2;
		const ry = shapeHeight / 2;
		const cx = shapeLeft + shapeWidth / 2;
		const cy = shapeTop + shapeHeight / 2;
		for (let index = 0; index < steps; index += 1) {
			const angle = -Math.PI / 2 + (Math.PI * 2 * index) / steps;
			const lobe =
				1.01 +
				Math.max(0, Math.sin(angle * 7 + 0.45)) * 0.08 +
				Math.max(0, Math.sin(angle * 13 - 0.8)) * 0.035;
			add(cx + Math.cos(angle) * rx * lobe, cy + Math.sin(angle) * ry * lobe);
		}
	} else if (shape === "burst") {
		const points = 36;
		const cx = shapeLeft + shapeWidth / 2;
		const cy = shapeTop + shapeHeight / 2;
		const spike = Math.min(width, height) * 0.16;
		const notch = Math.min(width, height) * 0.035;
		for (let index = 0; index < points; index += 1) {
			const angle = -Math.PI / 2 + (Math.PI * 2 * index) / points;
			const cos = Math.cos(angle);
			const sin = Math.sin(angle);
			const baseDistance = Math.min(
				Math.abs(cos) < 0.001 ? Number.POSITIVE_INFINITY : shapeWidth / 2 / Math.abs(cos),
				Math.abs(sin) < 0.001 ? Number.POSITIVE_INFINITY : shapeHeight / 2 / Math.abs(sin)
			);
			add(cx + cos * (baseDistance + (index % 2 === 0 ? spike : -notch)), cy + sin * baseDistance);
		}
	} else if (shape === "caption") {
		const step = Math.min(width, height) * 0.11;
		addPoints([
			point(step, 0),
			point(width - step * 0.55, 0),
			point(width, step * 0.55),
			point(width, height - step),
			point(width - step, height),
			point(step * 0.55, height),
			point(0, height - step * 0.55),
			point(0, step)
		]);
	} else if (shape === "terminal") {
		const cut = Math.min(width, height) * 0.14;
		const tab = Math.min(width, height) * 0.075;
		addPoints([
			point(cut, 0),
			point(width - tab, 0),
			point(width, tab),
			point(width, height - cut),
			point(width - cut, height),
			point(tab, height),
			point(0, height - tab),
			point(0, cut)
		]);
	} else if (shape === "ribbon") {
		const notch = Math.min(width, height) * 0.16;
		const pinch = Math.min(width, height) * 0.08;
		addPoints([
			point(pinch, 0),
			point(width - pinch, 0),
			point(width + notch, height / 2),
			point(width - pinch, height),
			point(pinch, height),
			point(-notch, height / 2)
		]);
	} else if (shape === "rectangle") {
		addPoints([point(0, 0), point(width, 0), point(width, height), point(0, height)]);
	} else {
		addRoundedRect(bubbleCornerRadiusForShape(shape));
	}

	const cumulative: number[] = [];
	let length = 0;
	for (let index = 0; index < samples.length; index += 1) {
		cumulative.push(length);
		length += pointDistance(samples[index], samples[(index + 1) % samples.length]);
	}
	return { samples: samples.length ? samples : [point(0, 0)], cumulative, length };
};

const normalizePerimeterDistance = (distance: number, total: number) =>
	total <= 0 ? 0 : ((distance % total) + total) % total;

const pointAtPerimeterDistance = (perimeter: Perimeter, distance: number) => {
	const normalized = normalizePerimeterDistance(distance, perimeter.length);
	for (let index = 0; index < perimeter.samples.length; index += 1) {
		const current = perimeter.samples[index];
		const next = perimeter.samples[(index + 1) % perimeter.samples.length];
		const segmentStart = perimeter.cumulative[index];
		const segmentLength = pointDistance(current, next);
		const segmentEnd = segmentStart + segmentLength;
		if (normalized <= segmentEnd || index === perimeter.samples.length - 1) {
			const progress = segmentLength === 0 ? 0 : (normalized - segmentStart) / segmentLength;
			return point(
				current.x + (next.x - current.x) * progress,
				current.y + (next.y - current.y) * progress
			);
		}
	}
	return perimeter.samples[0] ?? point(0, 0);
};

const closestPerimeterDistanceToLocalPoint = (perimeter: Perimeter, localTarget: Point) => {
	let bestDistance = 0;
	let bestScore = Number.POSITIVE_INFINITY;
	for (let index = 0; index < perimeter.samples.length; index += 1) {
		const current = perimeter.samples[index];
		const next = perimeter.samples[(index + 1) % perimeter.samples.length];
		const dx = next.x - current.x;
		const dy = next.y - current.y;
		const segmentLengthSquared = dx * dx + dy * dy;
		const progress =
			segmentLengthSquared === 0
				? 0
				: clamp(
						((localTarget.x - current.x) * dx + (localTarget.y - current.y) * dy) /
							segmentLengthSquared,
						0,
						1
					);
		const closest = point(current.x + dx * progress, current.y + dy * progress);
		const score = pointDistance(localTarget, closest);
		if (score < bestScore) {
			bestScore = score;
			bestDistance = perimeter.cumulative[index] + Math.sqrt(segmentLengthSquared) * progress;
		}
	}
	return bestDistance;
};

const perimeterPointsBetween = (perimeter: Perimeter, fromDistance: number, toDistance: number) => {
	const from = normalizePerimeterDistance(fromDistance, perimeter.length);
	let to = normalizePerimeterDistance(toDistance, perimeter.length);
	if (to <= from) to += perimeter.length;
	const span = to - from;
	const stepCount = Math.max(2, Math.ceil(span / 8));
	const points: Point[] = [];
	for (let index = 1; index < stepCount; index += 1) {
		points.push(pointAtPerimeterDistance(perimeter, from + span * (index / stepCount)));
	}
	return points;
};

const classListForStyle = (
	style: SpeechBubbleStyle,
	options: { wrapped: boolean; placeholder: boolean }
) =>
	[
		"hive-speech-bubble",
		`shape-${style.shape ?? "rounded"}`,
		`font-${style.fontFamily ?? "comic"}`,
		style.decoration ? `deco-${style.decoration}` : "",
		style.motion ? `motion-${style.motion}` : "",
		options.wrapped ? "wrapped" : "",
		options.placeholder ? "placeholder-mode" : ""
	]
		.filter(Boolean)
		.join(" ");

const createSvgElement = <Tag extends keyof SVGElementTagNameMap>(tag: Tag) =>
	document.createElementNS(SVG_NS, tag);

const applyStyleProperties = (element: HTMLElement, style: SpeechBubbleStyle) => {
	element.style.setProperty("--bubble-bg", style.background);
	element.style.setProperty("--bubble-border", style.border);
	element.style.setProperty("--bubble-text", style.text);
	element.style.setProperty("--bubble-shadow", style.shadow);
	for (const [key, value] of Object.entries(style.customProperties ?? {})) {
		element.style.setProperty(key.startsWith("--") ? key : `--${key}`, String(value));
	}
};

export const resolveBubbleLayoutBounds = ({
	viewport,
	maxWidthPercent = 35,
	maxHeightPercent = 25,
	minWidthPx = DEFAULT_BUBBLE_MIN_WIDTH_PX,
	maxWidthPx = DEFAULT_BUBBLE_MAX_WIDTH_PX,
	maxHeightPx
}: Pick<
	BubbleRendererOptions,
	"viewport" | "maxWidthPercent" | "maxHeightPercent" | "minWidthPx" | "maxWidthPx" | "maxHeightPx"
>): {
	viewport: Required<Viewport>;
	minWidthPx: number;
	maxWidthPx: number;
	maxHeightPx: number;
} => {
	const resolvedViewport = normalizeViewport(viewport);
	const resolvedMinWidthPx = Math.max(1, minWidthPx);
	// The wide mobile treatment is intended for a phone-sized page viewport. Embedded
	// landscape surfaces (for example, a video player) must keep the configured bounds.
	const isMobileViewport =
		resolvedViewport.width <= MOBILE_VIEWPORT_MAX_WIDTH_PX &&
		resolvedViewport.height > resolvedViewport.width;
	const isNarrowLandscapeViewport =
		resolvedViewport.width <= MOBILE_VIEWPORT_MAX_WIDTH_PX &&
		resolvedViewport.height <= resolvedViewport.width;
	const resolvedMaxWidthPx = isMobileViewport
		? resolvedViewport.width * (MOBILE_BUBBLE_MAX_WIDTH_PERCENT / 100)
		: isNarrowLandscapeViewport
			? Math.min(maxWidthPx, resolvedViewport.width * (clamp(maxWidthPercent, 12, 90) / 100))
			: Math.max(
					resolvedMinWidthPx,
					Math.min(maxWidthPx, resolvedViewport.width * (clamp(maxWidthPercent, 12, 90) / 100))
				);
	return {
		viewport: resolvedViewport,
		minWidthPx: Math.min(resolvedMinWidthPx, resolvedMaxWidthPx),
		maxWidthPx: resolvedMaxWidthPx,
		maxHeightPx: maxHeightPx ?? resolvedViewport.height * (clamp(maxHeightPercent, 10, 90) / 100)
	};
};

const measureLayoutOnce = ({
	text,
	style,
	scale,
	viewport,
	maxWidthPercent,
	maxHeightPercent,
	minWidthPx,
	maxWidthPx,
	maxHeightPx,
	fixedBodySize
}: {
	text: string;
	style: SpeechBubbleStyle;
	scale: number;
	viewport: Required<Viewport>;
	maxWidthPercent: number;
	maxHeightPercent: number;
	minWidthPx: number;
	maxWidthPx: number;
	maxHeightPx?: number;
	fixedBodySize?: Size;
}): TextLayout & { fits: boolean } => {
	const bounds = resolveBubbleLayoutBounds({
		viewport,
		maxWidthPercent,
		maxHeightPercent,
		minWidthPx,
		maxWidthPx,
		maxHeightPx
	});
	const fixedWidth =
		typeof fixedBodySize?.width === "number" && Number.isFinite(fixedBodySize.width)
			? Math.max(1, fixedBodySize.width)
			: null;
	const fixedHeight =
		typeof fixedBodySize?.height === "number" && Number.isFinite(fixedBodySize.height)
			? Math.max(1, fixedBodySize.height)
			: null;
	const metrics = chatBubbleTextMetrics({ scale, viewportWidthPx: viewport.width });
	const fontWeight = style.fontWeight ?? DEFAULT_CHAT_BUBBLE_FONT_WEIGHT;
	const font = `${fontWeight} ${metrics.fontSizePx}px ${fontFamilyForStyle(style)}`;
	const measure = (value: string) => measureTextWidth(value, font);
	const paddingX =
		metrics.fontSizePx * (style.paddingXEm ?? (style.shape === "ellipse" ? 4.35 : 1.05));
	const paddingY =
		metrics.fontSizePx * (style.paddingYEm ?? (style.shape === "ellipse" ? 2.2 : 0.9));

	if (style.shape === "ellipse") {
		const ellipse = layoutEllipseText({
			text,
			maxWidth: fixedWidth ?? bounds.maxWidthPx,
			maxHeight: fixedHeight ?? bounds.maxHeightPx,
			minWidth: fixedWidth ?? bounds.minWidthPx,
			lineHeight: metrics.lineHeightPx,
			scale,
			measureText: measure
		});
		return {
			width: fixedWidth ?? ellipse.width,
			height: fixedHeight ?? ellipse.height,
			lines: ellipse.lines.map((line) => line.text),
			ellipseLines: ellipse.lines,
			fontSizePx: metrics.fontSizePx,
			lineHeightPx: metrics.lineHeightPx,
			paddingX,
			paddingY,
			wrapped: ellipse.lines.length > 1,
			fits: fixedHeight !== null || ellipse.fits
		};
	}

	const maxInnerWidth = Math.max(40, bounds.maxWidthPx - paddingX * 2);
	const resolvedInnerWidth = fixedWidth ? Math.max(40, fixedWidth - paddingX * 2) : maxInnerWidth;
	const unwrappedWidth = measure(text);
	const wrapped = unwrappedWidth > resolvedInnerWidth;
	const lines = wrapped ? wrapText(text, resolvedInnerWidth, measure) : [text];
	const lineWidth = Math.max(...lines.map(measure), 1);
	const width = fixedWidth ?? clamp(lineWidth + paddingX * 2, bounds.minWidthPx, bounds.maxWidthPx);
	const naturalHeight =
		Math.max(metrics.lineHeightPx, lines.length * metrics.lineHeightPx) + paddingY * 2;
	const height =
		fixedHeight ??
		(naturalHeight > bounds.maxHeightPx + 0.5
			? naturalHeight
			: Math.min(bounds.maxHeightPx, naturalHeight));
	return {
		width,
		height,
		lines,
		fontSizePx: metrics.fontSizePx,
		lineHeightPx: metrics.lineHeightPx,
		paddingX,
		paddingY,
		wrapped,
		fits: fixedHeight !== null || naturalHeight <= bounds.maxHeightPx + 0.5
	};
};

const measureLayout = (options: MeasureLayoutParams): TextLayout => {
	const fixedHeight =
		typeof options.fixedBodySize?.height === "number" &&
		Number.isFinite(options.fixedBodySize.height);
	if (fixedHeight) return measureLayoutOnce(options);

	let best = measureLayoutOnce(options);
	if (best.fits) return best;

	const minScale = Math.max(0.2, options.scale * 0.62);
	let scale = options.scale;
	while (scale > minScale + 0.001) {
		scale = Math.max(minScale, scale * 0.9);
		const next = measureLayoutOnce({ ...options, scale });
		best = next;
		if (next.fits) return next;
	}

	return best;
};

export const measureBubbleBodySize = ({
	text,
	style,
	scale = 1,
	viewport,
	maxWidthPercent = 35,
	maxHeightPercent = 25,
	minWidthPx = DEFAULT_BUBBLE_MIN_WIDTH_PX,
	maxWidthPx = DEFAULT_BUBBLE_MAX_WIDTH_PX,
	maxHeightPx,
	fixedBodySize
}: Pick<
	BubbleRendererOptions,
	| "text"
	| "style"
	| "scale"
	| "viewport"
	| "maxWidthPercent"
	| "maxHeightPercent"
	| "minWidthPx"
	| "maxWidthPx"
	| "maxHeightPx"
	| "fixedBodySize"
>): Size => {
	const resolvedViewport = normalizeViewport(viewport);
	const resolvedStyle = resolveBubbleStyle(style);
	const resolvedScale = clamp(scale, 0.2, 3);
	const layout = measureLayout({
		text,
		style: resolvedStyle,
		scale: resolvedScale,
		viewport: resolvedViewport,
		maxWidthPercent: clamp(maxWidthPercent, 12, 90),
		maxHeightPercent: clamp(maxHeightPercent, 10, 90),
		minWidthPx,
		maxWidthPx,
		maxHeightPx,
		fixedBodySize
	});
	return {
		width: layout.width,
		height: layout.height
	};
};

class DomBubbleRenderer implements BubbleRenderer {
	readonly element: HTMLDivElement;
	#svg: SVGSVGElement;
	#path: SVGPathElement;
	#debugSvg: SVGSVGElement;
	#content: HTMLDivElement;
	#target: HTMLElement;
	#options: Required<
		Pick<
			BubbleRendererOptions,
			| "text"
			| "pose"
			| "scale"
			| "maxWidthPercent"
			| "maxHeightPercent"
			| "minWidthPx"
			| "maxWidthPx"
			| "zIndex"
			| "placeholder"
			| "debug"
			| "constrainToViewport"
		>
	> &
		Pick<
			BubbleRendererOptions,
			| "id"
			| "style"
			| "target"
			| "maxHeightPx"
			| "fixedBodySize"
			| "fixedBodyRect"
			| "viewport"
			| "onDebug"
		>;
	#debug: BubbleDebug | null = null;
	#scheduler: PausableScheduler;
	#ownsScheduler: boolean;
	#paused = false;
	#hideTimer: PausableTimer | null = null;
	#hidePromise: Promise<void> | null = null;
	#hideResolve: (() => void) | null = null;
	#showTimer: PausableTimer | null = null;
	#disposed = false;

	constructor(options: BubbleRendererOptions) {
		if (typeof document === "undefined") {
			throw new Error("@hivetech/speech-bubbles requires a browser DOM.");
		}
		this.#target = options.target ?? document.body;
		this.#scheduler = options.scheduler ?? new PausableScheduler();
		this.#ownsScheduler = !options.scheduler;
		this.#options = {
			id: options.id,
			target: options.target,
			text: options.text,
			pose: options.pose,
			style: options.style,
			scale: options.scale ?? 1,
			maxWidthPercent: options.maxWidthPercent ?? 35,
			maxHeightPercent: options.maxHeightPercent ?? 25,
			minWidthPx: options.minWidthPx ?? DEFAULT_BUBBLE_MIN_WIDTH_PX,
			maxWidthPx: options.maxWidthPx ?? DEFAULT_BUBBLE_MAX_WIDTH_PX,
			maxHeightPx: options.maxHeightPx,
			fixedBodySize: options.fixedBodySize,
			fixedBodyRect: options.fixedBodyRect,
			zIndex: options.zIndex ?? DEFAULT_Z_INDEX,
			placeholder: options.placeholder ?? false,
			debug: options.debug ?? false,
			constrainToViewport: options.constrainToViewport ?? true,
			viewport: options.viewport,
			onDebug: options.onDebug
		};

		this.element = document.createElement("div");
		this.element.style.position = this.#target === document.body ? "fixed" : "absolute";
		this.#svg = createSvgElement("svg");
		this.#path = createSvgElement("path");
		this.#debugSvg = createSvgElement("svg");
		this.#content = document.createElement("div");
		this.#svg.classList.add("hive-speech-bubble__shell");
		this.#debugSvg.classList.add("hive-speech-bubble__debug");
		this.#content.classList.add("hive-speech-bubble__content");
		this.#svg.append(this.#path);
		this.element.append(this.#svg, this.#debugSvg, this.#content);
		if (this.#options.id) this.element.dataset.bubbleId = this.#options.id;
		this.#target.append(this.element);
		this.#render();
	}

	get debug() {
		return this.#debug;
	}

	#settleHide() {
		this.#scheduler.clearTimeout(this.#hideTimer);
		this.#hideTimer = null;
		const resolve = this.#hideResolve;
		this.#hideResolve = null;
		this.#hidePromise = null;
		resolve?.();
	}

	update(options: BubbleRendererUpdateOptions) {
		this.#options = { ...this.#options, ...options };
		this.#render();
	}

	setPaused(paused: boolean) {
		this.#paused = paused;
		this.element.classList.toggle("is-playback-paused", paused);
		if (this.#ownsScheduler) this.#scheduler.setPaused(paused);
	}

	show() {
		if (this.#disposed) return;
		if (this.#hidePromise) this.#settleHide();
		if (this.#showTimer) {
			this.#scheduler.clearTimeout(this.#showTimer);
			this.#showTimer = null;
		}
		this.element.classList.remove("leaving");
		this.#showTimer = this.#scheduler.setTimeout(() => {
			this.#showTimer = null;
			if (this.#disposed) return;
			this.element.classList.add("entering", "is-visible");
			this.#showTimer = this.#scheduler.setTimeout(() => {
				this.#showTimer = null;
				if (!this.#disposed) this.element.classList.remove("entering");
			}, BUBBLE_TRANSITION_MS);
		}, 0);
	}

	hide(options?: BubbleRendererHideOptions) {
		if (this.#disposed) return Promise.resolve();
		if (this.#showTimer) {
			this.#scheduler.clearTimeout(this.#showTimer);
			this.#showTimer = null;
		}
		if (options?.animate === false) {
			if (this.#hidePromise) this.#settleHide();
			if (this.#hideTimer) {
				this.#scheduler.clearTimeout(this.#hideTimer);
				this.#hideTimer = null;
			}
			this.element.classList.remove("entering", "leaving", "is-visible");
			this.element.style.opacity = "0";
			return Promise.resolve();
		}
		if (!this.#hidePromise) {
			this.#hidePromise = new Promise<void>((resolve) => {
				this.#hideResolve = resolve;
			});
		}
		this.#scheduler.clearTimeout(this.#hideTimer);
		this.element.classList.remove("entering");
		this.element.classList.add("leaving");
		this.element.classList.remove("is-visible");
		this.#hideTimer = this.#scheduler.setTimeout(() => this.#settleHide(), BUBBLE_TRANSITION_MS);
		return this.#hidePromise;
	}

	dispose() {
		this.#disposed = true;
		if (this.#showTimer) {
			this.#scheduler.clearTimeout(this.#showTimer);
			this.#showTimer = null;
		}
		if (this.#hidePromise) this.#settleHide();
		else if (this.#hideTimer) {
			this.#scheduler.clearTimeout(this.#hideTimer);
			this.#hideTimer = null;
		}
		this.element.remove();
		if (this.#ownsScheduler) this.#scheduler.dispose();
	}

	#render() {
		const viewport = normalizeViewport(this.#options.viewport);
		const style = resolveBubbleStyle(this.#options.style);
		const shape = style.shape ?? "rounded";
		const scale = clamp(this.#options.scale, 0.2, 3);
		const layout = measureLayout({
			text: this.#options.text,
			style,
			scale,
			viewport,
			maxWidthPercent: clamp(this.#options.maxWidthPercent, 12, 90),
			maxHeightPercent: clamp(this.#options.maxHeightPercent, 10, 90),
			minWidthPx: Math.max(1, this.#options.minWidthPx),
			maxWidthPx: Math.max(this.#options.minWidthPx, this.#options.maxWidthPx),
			maxHeightPx: this.#options.maxHeightPx,
			fixedBodySize: this.#options.fixedBodySize
		});
		const perimeter = shapePerimeter(layout.width, layout.height, shape);
		const pose = this.#options.pose;
		const subject = subjectCircleForPose(pose);
		const direction = normalizeVector(pose.tailVector);
		const desiredAnchorLocal = point(
			layout.width / 2 + direction.x * Math.max(layout.width, layout.height) * 2,
			layout.height / 2 + direction.y * Math.max(layout.width, layout.height) * 2
		);
		const anchorDistance = closestPerimeterDistanceToLocalPoint(perimeter, desiredAnchorLocal);
		let baseCenter = pointAtPerimeterDistance(perimeter, anchorDistance);
		let contentLeft = pose.anchor.x - baseCenter.x;
		let contentTop = pose.anchor.y - baseCenter.y;
		if (this.#options.fixedBodyRect) {
			contentLeft = this.#options.fixedBodyRect.left;
			contentTop = this.#options.fixedBodyRect.top;
		}

		if (this.#options.constrainToViewport && !this.#options.fixedBodyRect) {
			const margin = 8;
			const minLeft = viewport.left + margin;
			const maxRight = viewport.left + viewport.width - margin;
			const minTop = viewport.top + margin;
			const maxBottom = viewport.top + viewport.height - margin;
			if (contentLeft < minLeft) contentLeft = minLeft;
			else if (contentLeft + layout.width > maxRight) contentLeft = maxRight - layout.width;
			if (contentTop < minTop) contentTop = minTop;
			else if (contentTop + layout.height > maxBottom) contentTop = maxBottom - layout.height;
		}

		// The body can be pushed a long way from its authored track when it is constrained
		// to a smaller viewport. Re-aim from the rendered body toward the subject center;
		// preserving the old edge point can leave a tail that touches the subject circle
		// tangentially without actually pointing at the speaker.
		const targetSubjectCenterLocal = point(
			subject.center.x - contentLeft,
			subject.center.y - contentTop
		);
		const renderedAnchorDistance = closestPerimeterDistanceToLocalPoint(
			perimeter,
			targetSubjectCenterLocal
		);
		baseCenter = pointAtPerimeterDistance(perimeter, renderedAnchorDistance);
		const rawDx = subject.center.x - (contentLeft + baseCenter.x);
		const rawDy = subject.center.y - (contentTop + baseCenter.y);
		const distance = Math.max(0, Math.hypot(rawDx, rawDy));
		const renderedDirection = normalizeVector({ x: rawDx, y: rawDy }, direction);
		const tailLength = Math.max(0, distance - subject.radius);
		const tailTipLocal = point(
			baseCenter.x + renderedDirection.x * tailLength,
			baseCenter.y + renderedDirection.y * tailLength
		);
		const tailTip = point(contentLeft + tailTipLocal.x, contentTop + tailTipLocal.y);
		const targetInsideContent =
			tailTip.x >= contentLeft &&
			tailTip.x <= contentLeft + layout.width &&
			tailTip.y >= contentTop &&
			tailTip.y <= contentTop + layout.height;
		const baseWidth = resolveTailBaseWidth({
			tailLength,
			scale,
			perimeterLength: perimeter.length
		});
		const baseA = pointAtPerimeterDistance(perimeter, renderedAnchorDistance - baseWidth / 2);
		const baseB = pointAtPerimeterDistance(perimeter, renderedAnchorDistance + baseWidth / 2);
		const tailVisible = !targetInsideContent && (style.tail ?? "center") !== "none";
		const path = tailVisible
			? [
					baseB,
					...perimeterPointsBetween(
						perimeter,
						renderedAnchorDistance + baseWidth / 2,
						renderedAnchorDistance - baseWidth / 2
					),
					baseA,
					tailTipLocal
				]
					.map((entry, index) => `${index === 0 ? "M" : "L"} ${entry.x} ${entry.y}`)
					.join(" ") + " Z"
			: perimeter.samples
					.map((entry, index) => `${index === 0 ? "M" : "L"} ${entry.x} ${entry.y}`)
					.join(" ") + " Z";
		const shapeXs = perimeter.samples.map((entry) => entry.x);
		const shapeYs = perimeter.samples.map((entry) => entry.y);
		const xs = tailVisible
			? [0, layout.width, tailTipLocal.x, ...shapeXs]
			: [0, layout.width, ...shapeXs];
		const ys = tailVisible
			? [0, layout.height, tailTipLocal.y, ...shapeYs]
			: [0, layout.height, ...shapeYs];
		const shellLeft = Math.min(...xs) - SHELL_PADDING;
		const shellTop = Math.min(...ys) - SHELL_PADDING;
		const shellRight = Math.max(...xs) + SHELL_PADDING;
		const shellBottom = Math.max(...ys) + SHELL_PADDING;
		const shell = rect(
			Math.round((contentLeft + shellLeft) * 100) / 100,
			Math.round((contentTop + shellTop) * 100) / 100,
			Math.round(Math.max(1, shellRight - shellLeft) * 100) / 100,
			Math.round(Math.max(1, shellBottom - shellTop) * 100) / 100
		);
		const content = rect(
			Math.round(contentLeft * 100) / 100,
			Math.round(contentTop * 100) / 100,
			Math.round(layout.width * 100) / 100,
			Math.round(layout.height * 100) / 100
		);
		const transitionOrigin = tailVisible ? tailTipLocal : baseCenter;

		this.element.className = classListForStyle(style, {
			wrapped: layout.wrapped,
			placeholder: this.#options.placeholder
		});
		this.element.classList.toggle("is-playback-paused", this.#paused);
		this.element.style.left = `${shell.left}px`;
		this.element.style.top = `${shell.top}px`;
		this.element.style.width = `${shell.width}px`;
		this.element.style.height = `${shell.height}px`;
		this.element.style.zIndex = String(this.#options.zIndex);
		this.element.style.setProperty("--bubble-origin-x", `${transitionOrigin.x - shellLeft}px`);
		this.element.style.setProperty("--bubble-origin-y", `${transitionOrigin.y - shellTop}px`);
		this.element.style.setProperty("--bubble-scale", String(scale));
		this.element.style.setProperty("--bubble-body-left", `${-shellLeft}px`);
		this.element.style.setProperty("--bubble-body-top", `${-shellTop}px`);
		this.element.style.setProperty("--bubble-body-width", `${layout.width}px`);
		this.element.style.setProperty("--bubble-body-height", `${layout.height}px`);
		this.element.style.setProperty(
			"--bubble-body-radius",
			`${bubbleCornerRadiusForShape(shape)}px`
		);
		applyStyleProperties(this.element, style);

		this.#svg.setAttribute("viewBox", `${shellLeft} ${shellTop} ${shell.width} ${shell.height}`);
		this.#path.setAttribute("d", path);
		this.#path.style.strokeWidth = `${style.borderWidthPx ?? (shape === "terminal" ? 2.5 : shape === "burst" ? 4 : 3)}px`;
		this.#renderContent(layout, style, shellLeft, shellTop);
		this.#renderDebug({
			enabled: this.#options.debug,
			shellLeft,
			shellTop,
			shell,
			contentLeft,
			contentTop,
			baseCenter,
			baseA,
			baseB,
			tailTipLocal
		});

		this.#debug = {
			contentRect: content,
			shellRect: shell,
			anchor: point(contentLeft + baseCenter.x, contentTop + baseCenter.y),
			tailTip: point(contentLeft + tailTipLocal.x, contentTop + tailTipLocal.y),
			subjectCircle: subject,
			trackCircle: trackCircleForPose(pose),
			exposedTailLength: tailLength,
			overlapsSubject: targetInsideContent,
			tailVisible,
			path
		};
		this.#options.onDebug?.(this.#debug);
	}

	#renderContent(
		layout: TextLayout,
		style: SpeechBubbleStyle,
		shellLeft: number,
		shellTop: number
	) {
		this.#content.replaceChildren();
		this.#content.style.left = `${-shellLeft}px`;
		this.#content.style.top = `${-shellTop}px`;
		this.#content.style.width = `${layout.width}px`;
		this.#content.style.height = `${layout.height}px`;
		this.#content.style.fontFamily = fontFamilyForStyle(style);
		this.#content.style.fontSize = `${layout.fontSizePx}px`;
		this.#content.style.lineHeight = `${layout.lineHeightPx}px`;
		this.#content.style.fontWeight = String(style.fontWeight ?? DEFAULT_CHAT_BUBBLE_FONT_WEIGHT);
		const hasFixedBodySize = Boolean(this.#options.fixedBodySize);
		const visibleLines = visibleTextForLayoutLines(this.#options.text, layout.lines);
		if (layout.ellipseLines) {
			this.#content.classList.add("shape-aware");
			this.#content.style.display = "";
			this.#content.style.alignItems = "";
			this.#content.style.justifyContent = "";
			this.#content.style.padding = "0";
			for (const [index, line] of layout.ellipseLines.entries()) {
				const span = document.createElement("span");
				span.className = "hive-speech-bubble__line";
				span.textContent = visibleLines[index] ?? "";
				span.style.left = `${line.left}px`;
				span.style.top = `${line.top}px`;
				span.style.width = `${line.width}px`;
				this.#content.append(span);
			}
			return;
		}
		this.#content.classList.remove("shape-aware");
		this.#content.style.display = hasFixedBodySize ? "flex" : "";
		this.#content.style.flexDirection = hasFixedBodySize ? "column" : "";
		this.#content.style.alignItems = hasFixedBodySize ? "center" : "";
		this.#content.style.justifyContent = hasFixedBodySize ? "center" : "";
		this.#content.style.padding = `${layout.paddingY}px ${layout.paddingX}px`;
		this.#content.style.whiteSpace = "pre";
		for (const line of visibleLines) {
			const span = document.createElement("span");
			span.className = "hive-speech-bubble__text-line";
			span.textContent = line;
			span.style.height = `${layout.lineHeightPx}px`;
			this.#content.append(span);
		}
	}

	#renderDebug({
		enabled,
		shellLeft,
		shellTop,
		shell,
		contentLeft,
		contentTop,
		baseCenter,
		baseA,
		baseB,
		tailTipLocal
	}: {
		enabled: boolean;
		shellLeft: number;
		shellTop: number;
		shell: Rect;
		contentLeft: number;
		contentTop: number;
		baseCenter: Point;
		baseA: Point;
		baseB: Point;
		tailTipLocal: Point;
	}) {
		this.#debugSvg.replaceChildren();
		this.#debugSvg.style.display = enabled ? "" : "none";
		if (!enabled) return;
		this.#debugSvg.setAttribute(
			"viewBox",
			`${shellLeft} ${shellTop} ${shell.width} ${shell.height}`
		);
		const subject = subjectCircleForPose(this.#options.pose);
		const track = trackCircleForPose(this.#options.pose);
		const subjectCenter = point(subject.center.x - contentLeft, subject.center.y - contentTop);
		const trackCenter = point(track.center.x - contentLeft, track.center.y - contentTop);
		const trackCircle = createSvgElement("circle");
		trackCircle.classList.add("hive-speech-bubble__debug-track");
		trackCircle.setAttribute("cx", String(trackCenter.x));
		trackCircle.setAttribute("cy", String(trackCenter.y));
		trackCircle.setAttribute("r", String(track.radius));
		const subjectCircle = createSvgElement("circle");
		subjectCircle.classList.add("hive-speech-bubble__debug-subject");
		subjectCircle.setAttribute("cx", String(subjectCenter.x));
		subjectCircle.setAttribute("cy", String(subjectCenter.y));
		subjectCircle.setAttribute("r", String(subject.radius));
		const line = createSvgElement("line");
		line.classList.add("hive-speech-bubble__debug-tail-base");
		line.setAttribute("x1", String(baseA.x));
		line.setAttribute("y1", String(baseA.y));
		line.setAttribute("x2", String(baseB.x));
		line.setAttribute("y2", String(baseB.y));
		const base = createSvgElement("circle");
		base.classList.add("hive-speech-bubble__debug-anchor");
		base.setAttribute("cx", String(baseCenter.x));
		base.setAttribute("cy", String(baseCenter.y));
		base.setAttribute("r", "4");
		const tip = createSvgElement("circle");
		tip.classList.add("hive-speech-bubble__debug-tail-tip");
		tip.setAttribute("cx", String(tailTipLocal.x));
		tip.setAttribute("cy", String(tailTipLocal.y));
		tip.setAttribute("r", "4");
		this.#debugSvg.append(trackCircle, subjectCircle, line, base, tip);
	}
}

export const createBubbleRenderer = (options: BubbleRendererOptions): BubbleRenderer =>
	new DomBubbleRenderer(options);
