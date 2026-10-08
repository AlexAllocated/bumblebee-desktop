import {
	createBubbleLayer,
	createPoseFromSubject,
	chatBubbleTextMetrics,
	DEFAULT_BUBBLE_MAX_WIDTH_PX,
	DEFAULT_BUBBLE_MIN_WIDTH_PX,
	type BubbleHideOptions,
	type BubbleLayer,
	type BubblePageSnapshot,
	type BubblePose,
	type SpeechBubbleStyle,
	type SpeechTimeline,
	type BubbleUpdateOptions,
	type Viewport
} from "@hivetech/speech-bubbles";
import { createActor } from "xstate";
import { chatBubbleLayerMachine } from "./machines";

export type { BubblePose, SpeechBubbleStyle, SpeechTimeline };
export type ChatBubblePage = BubblePageSnapshot;
export type BubblePresentationUpdate = Omit<BubbleUpdateOptions, "pose" | "text">;

export type BubbleScreenRect = {
	left: number;
	right: number;
	top: number;
	bottom: number;
	width: number;
	height: number;
	centerX: number;
	centerY: number;
};

export type BubbleOptions = {
	enabled?: boolean;
	container?: HTMLElement | null;
	maxWidthPercent?: number;
	maxHeightPercent?: number;
	scale?: number;
	designViewport?: { width: number; height: number } | null;
	zIndex?: number;
	debug?: boolean;
	viewport?: Viewport | (() => Viewport);
};

export type BubbleDesignViewport = { width: number; height: number };

export type BubblePayload = {
	audioId: string;
	text: string;
	timeline?: SpeechTimeline | null;
	style?: SpeechBubbleStyle | null;
	subject?: "puppet" | "bumblebee" | "streamerVoiceBubble";
	puppetId?: string | null;
	actorId?: string | null;
	exitAnimation?: boolean;
	viewport?: Viewport | (() => Viewport);
};

type ChatBubbleLifecycle = {
	onPageExit?: (page: BubblePageSnapshot) => void;
};

type BubbleStartOptions = {
	autoReveal?: boolean;
	manualReveal?: boolean;
	audioDurationMs?: number;
};

const ATTACHED_CHAT_BUBBLE_MIN_TEXT_SCALE = 0.5;
const ATTACHED_CHAT_BUBBLE_TEXT_SCALE_MULTIPLIER = 2;
const DEFAULT_ATTACHED_CHAT_BUBBLE_SCALE = 0.5;
const DEFAULT_MAX_WIDTH_PERCENT = 35;
const DEFAULT_MAX_HEIGHT_PERCENT = 25;
const DEFAULT_Z_INDEX = 2147483000;

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

const finitePositive = (value: unknown): value is number =>
	typeof value === "number" && Number.isFinite(value) && value > 0;

const defaultViewport = (): Required<Viewport> => ({
	left: 0,
	top: 0,
	width:
		typeof window !== "undefined" && finitePositive(window.innerWidth) ? window.innerWidth : 1280,
	height:
		typeof window !== "undefined" && finitePositive(window.innerHeight) ? window.innerHeight : 720
});

const resolveViewport = (viewport?: Viewport | (() => Viewport)): Required<Viewport> => {
	const fallback = defaultViewport();
	const resolved = typeof viewport === "function" ? viewport() : viewport;
	return {
		left: Number.isFinite(resolved?.left) ? (resolved?.left ?? 0) : fallback.left,
		top: Number.isFinite(resolved?.top) ? (resolved?.top ?? 0) : fallback.top,
		width: finitePositive(resolved?.width) ? resolved.width : fallback.width,
		height: finitePositive(resolved?.height) ? resolved.height : fallback.height
	};
};

const normalizePercent = (value: unknown, fallback: number, min: number, max: number) =>
	typeof value === "number" && Number.isFinite(value) ? clamp(value, min, max) : fallback;

const normalizeScale = (value: unknown) =>
	typeof value === "number" && Number.isFinite(value)
		? clamp(value, 0.2, 2.5)
		: DEFAULT_ATTACHED_CHAT_BUBBLE_SCALE;

const normalizeDesignViewport = (value: unknown): BubbleDesignViewport | undefined => {
	if (!value || typeof value !== "object") return undefined;
	const width = Number((value as { width?: unknown }).width);
	const height = Number((value as { height?: unknown }).height);
	return finitePositive(width) && finitePositive(height) ? { width, height } : undefined;
};

export const resolveBubbleDesignFitScale = ({
	viewport,
	designViewport
}: {
	viewport: Viewport;
	designViewport?: BubbleDesignViewport;
}) => {
	if (!designViewport) return 1;
	const resolvedViewport = resolveViewport(viewport);
	return Math.min(
		resolvedViewport.width / designViewport.width,
		resolvedViewport.height / designViewport.height
	);
};

export const resolveBubbleDisplayScale = ({
	authoredScale,
	viewport,
	designViewport
}: {
	authoredScale: number;
	viewport: Viewport;
	designViewport?: BubbleDesignViewport;
}) => {
	const baseScale = Math.max(
		ATTACHED_CHAT_BUBBLE_MIN_TEXT_SCALE,
		authoredScale * ATTACHED_CHAT_BUBBLE_TEXT_SCALE_MULTIPLIER
	);
	if (!designViewport) return baseScale;
	const resolvedViewport = resolveViewport(viewport);
	const fitScale = resolveBubbleDesignFitScale({ viewport: resolvedViewport, designViewport });
	const localFontSize = chatBubbleTextMetrics({
		viewportWidthPx: resolvedViewport.width
	}).fontSizePx;
	const designFontSize = chatBubbleTextMetrics({
		viewportWidthPx: designViewport.width
	}).fontSizePx;
	return baseScale * fitScale * (designFontSize / localFontSize);
};

const normalizeRect = (rect: BubbleScreenRect | null | undefined): BubbleScreenRect | null => {
	if (!rect) return null;
	const left = Number(rect.left);
	const right = Number(rect.right);
	const top = Number(rect.top);
	const bottom = Number(rect.bottom);
	const width = Number(rect.width ?? right - left);
	const height = Number(rect.height ?? bottom - top);
	const centerX = Number(rect.centerX ?? (left + right) / 2);
	const centerY = Number(rect.centerY ?? (top + bottom) / 2);
	if (![left, right, top, bottom, width, height, centerX, centerY].every(Number.isFinite)) {
		return null;
	}
	return { left, right, top, bottom, width, height, centerX, centerY };
};

export const localizeBubbleScreenRect = (
	rect: BubbleScreenRect,
	container: HTMLElement
): BubbleScreenRect => {
	const containerRect = container.getBoundingClientRect();
	return {
		...rect,
		left: rect.left - containerRect.left,
		right: rect.right - containerRect.left,
		top: rect.top - containerRect.top,
		bottom: rect.bottom - containerRect.top,
		centerX: rect.centerX - containerRect.left,
		centerY: rect.centerY - containerRect.top
	};
};

const queueKeyForPayload = (payload: BubblePayload) =>
	payload.actorId ||
	payload.puppetId ||
	(payload.subject ? `subject:${payload.subject}` : "subject:bumblebee");

export class ChatBubbles {
	readonly actorRef = createActor(chatBubbleLayerMachine).start();
	enabled: boolean;
	maxWidthPercent: number;
	maxHeightPercent: number;
	scale: number;
	designViewport?: BubbleDesignViewport;
	zIndex: number;
	debug: boolean;
	viewport?: Viewport | (() => Viewport);
	#layer: BubbleLayer | null = null;
	#rects = new Map<string, BubbleScreenRect>();
	#viewports = new Map<string, Viewport | (() => Viewport)>();
	#lifecycle: ChatBubbleLifecycle;
	#container: HTMLElement | null;
	#paused = false;

	constructor(options?: BubbleOptions | boolean, lifecycle: ChatBubbleLifecycle = {}) {
		this.#lifecycle = lifecycle;
		const resolved = typeof options === "boolean" ? { enabled: options } : (options ?? {});
		this.#container = resolved.container ?? null;
		this.enabled = resolved.enabled ?? true;
		this.maxWidthPercent = normalizePercent(
			resolved.maxWidthPercent,
			DEFAULT_MAX_WIDTH_PERCENT,
			12,
			90
		);
		this.maxHeightPercent = normalizePercent(
			resolved.maxHeightPercent,
			DEFAULT_MAX_HEIGHT_PERCENT,
			10,
			90
		);
		this.scale = normalizeScale(resolved.scale);
		this.designViewport = normalizeDesignViewport(resolved.designViewport);
		this.zIndex = Math.floor(resolved.zIndex ?? DEFAULT_Z_INDEX);
		this.debug = resolved.debug ?? false;
		this.viewport = resolved.viewport;
		if (this.enabled) {
			this.#layer = this.#createLayer();
			this.actorRef.send({ type: "ENABLE" });
		}
	}

	start(
		payload: BubblePayload | null | undefined,
		rect: BubbleScreenRect | null,
		options?: BubbleStartOptions
	) {
		if (!this.enabled || !this.#layer || !payload?.audioId || !payload.text.trim()) return;
		const screenRect = normalizeRect(rect);
		const normalizedRect = screenRect
			? this.#container
				? localizeBubbleScreenRect(screenRect, this.#container)
				: screenRect
			: null;
		if (!normalizedRect) return;
		const cleanPayload = { ...payload, text: payload.text.trim() };
		this.#rects.set(cleanPayload.audioId, normalizedRect);
		const bubbleViewport = cleanPayload.viewport ?? this.viewport;
		const geometryScale = this.#geometryScale(bubbleViewport);
		if (bubbleViewport) this.#viewports.set(cleanPayload.audioId, bubbleViewport);
		this.actorRef.send({ type: "START" });
		this.#layer.show({
			id: cleanPayload.audioId,
			text: cleanPayload.text,
			timeline: cleanPayload.timeline ?? null,
			style: cleanPayload.style ?? null,
			pose: this.#poseFor(normalizedRect, bubbleViewport),
			viewport: bubbleViewport,
			queueKey: queueKeyForPayload(cleanPayload),
			autoReveal: options?.autoReveal,
			manualReveal: options?.manualReveal,
			audioDurationMs: options?.audioDurationMs,
			scale: this.#displayScale(bubbleViewport),
			minWidthPx: DEFAULT_BUBBLE_MIN_WIDTH_PX * geometryScale,
			maxWidthPx: DEFAULT_BUBBLE_MAX_WIDTH_PX * geometryScale,
			maxWidthPercent: this.maxWidthPercent,
			maxHeightPercent: this.maxHeightPercent,
			zIndex: this.zIndex + 1,
			debug: this.debug,
			animatePageExit: cleanPayload.exitAnimation
		});
	}

	reveal(audioId: string, text: string, chunkIndex = 0, fullCursor?: number) {
		this.#layer?.reveal(audioId, text, { chunkIndex, fullCursor });
	}

	getPage(audioId: string) {
		return this.#layer?.getPage(audioId) ?? null;
	}

	updateGeometry(audioId: string, rect: BubbleScreenRect | null) {
		const screenRect = normalizeRect(rect);
		const normalizedRect = screenRect
			? this.#container
				? localizeBubbleScreenRect(screenRect, this.#container)
				: screenRect
			: null;
		if (!this.#layer || !normalizedRect) return;
		this.#rects.set(audioId, normalizedRect);
		this.#layer.update(audioId, {
			pose: this.#poseFor(normalizedRect, this.#viewports.get(audioId))
		});
	}

	updateStyle(audioId: string, style: SpeechBubbleStyle | null) {
		this.#layer?.update(audioId, { style });
	}

	setPaused(paused: boolean) {
		this.#paused = paused;
		this.#layer?.setPaused?.(paused);
	}

	updatePresentation(audioId: string, options: BubblePresentationUpdate) {
		if (!this.#layer) return;
		if ("viewport" in options) {
			if (options.viewport) this.#viewports.set(audioId, options.viewport);
			else this.#viewports.delete(audioId);
		}
		const rect = this.#rects.get(audioId);
		const bubbleViewport = this.#viewports.get(audioId) ?? this.viewport;
		this.#layer.update(audioId, {
			...options,
			...(rect ? { pose: this.#poseFor(rect, bubbleViewport) } : {})
		});
	}

	updateOptions(options?: BubbleOptions | boolean) {
		const resolved = typeof options === "boolean" ? { enabled: options } : (options ?? {});
		const nextEnabled = resolved.enabled ?? this.enabled;
		this.maxWidthPercent = normalizePercent(
			resolved.maxWidthPercent ?? this.maxWidthPercent,
			DEFAULT_MAX_WIDTH_PERCENT,
			12,
			90
		);
		this.maxHeightPercent = normalizePercent(
			resolved.maxHeightPercent ?? this.maxHeightPercent,
			DEFAULT_MAX_HEIGHT_PERCENT,
			10,
			90
		);
		this.scale = normalizeScale(resolved.scale ?? this.scale);
		if ("designViewport" in resolved) {
			this.designViewport = normalizeDesignViewport(resolved.designViewport);
		}
		this.zIndex = Math.floor(resolved.zIndex ?? this.zIndex);
		this.debug = resolved.debug ?? this.debug;
		if ("viewport" in resolved) this.viewport = resolved.viewport;
		if ("container" in resolved) this.setContainer(resolved.container);

		if (nextEnabled && !this.#layer) {
			this.#layer = this.#createLayer();
			this.actorRef.send({ type: "ENABLE" });
		}
		if (!nextEnabled && this.#layer) {
			this.#layer.dispose();
			this.#layer = null;
			this.#rects.clear();
			this.#viewports.clear();
		}
		this.enabled = nextEnabled;

		if (!this.#layer) return;
		for (const [audioId, rect] of this.#rects) {
			const bubbleViewport = this.#viewports.get(audioId) ?? this.viewport;
			const geometryScale = this.#geometryScale(bubbleViewport);
			this.#layer.update(audioId, {
				pose: this.#poseFor(rect, bubbleViewport),
				scale: this.#displayScale(bubbleViewport),
				minWidthPx: DEFAULT_BUBBLE_MIN_WIDTH_PX * geometryScale,
				maxWidthPx: DEFAULT_BUBBLE_MAX_WIDTH_PX * geometryScale,
				maxWidthPercent: this.maxWidthPercent,
				maxHeightPercent: this.maxHeightPercent,
				zIndex: this.zIndex + 1,
				debug: this.debug
			});
		}
	}

	setContainer(container?: HTMLElement | null) {
		const nextContainer = container ?? null;
		if (nextContainer === this.#container) return;
		if (this.#layer?.sessions.length) {
			throw new Error("Chat bubble container cannot change while a bubble is active.");
		}
		this.#container = nextContainer;
		this.#layer?.dispose();
		this.#layer = this.enabled ? this.#createLayer() : null;
	}

	async end(audioId: string, options?: BubbleHideOptions) {
		if (!this.#layer) return;
		await this.#layer.hide(audioId, options);
		this.#rects.delete(audioId);
		this.#viewports.delete(audioId);
		this.actorRef.send({ type: "END" });
		if (this.#layer.sessions.length === 0) this.actorRef.send({ type: "IDLE" });
	}

	clear() {
		this.#layer?.dispose();
		this.#layer = this.enabled ? this.#createLayer() : null;
		this.#rects.clear();
		this.#viewports.clear();
		this.actorRef.send({ type: "IDLE" });
	}

	dispose() {
		this.actorRef.send({ type: "DISPOSE" });
		this.#layer?.dispose();
		this.#layer = null;
		this.#rects.clear();
		this.#viewports.clear();
		this.actorRef.stop();
	}

	#createLayer() {
		const layer = createBubbleLayer({
			target: this.#container ?? undefined,
			zIndex: this.zIndex,
			debug: this.debug,
			viewport: () => resolveViewport(this.viewport),
			onPageExit: this.#lifecycle.onPageExit
		});
		layer.setPaused?.(this.#paused);
		return layer;
	}

	#displayScale(bubbleViewport?: Viewport | (() => Viewport)) {
		return resolveBubbleDisplayScale({
			authoredScale: this.scale,
			viewport: resolveViewport(bubbleViewport ?? this.viewport),
			designViewport: this.designViewport
		});
	}

	#geometryScale(bubbleViewport?: Viewport | (() => Viewport)) {
		return resolveBubbleDesignFitScale({
			viewport: resolveViewport(bubbleViewport ?? this.viewport),
			designViewport: this.designViewport
		});
	}

	#poseFor(rect: BubbleScreenRect, bubbleViewport?: Viewport | (() => Viewport)): BubblePose {
		const viewport = resolveViewport(bubbleViewport ?? this.viewport);
		const geometryScale = this.#geometryScale(viewport);
		const subjectRadius = clamp(
			Math.max(rect.width, rect.height) / 2,
			28 * geometryScale,
			220 * geometryScale
		);
		const tailLength = clamp(subjectRadius * 0.38, 32 * geometryScale, 96 * geometryScale);
		const viewportCenterX = viewport.left + viewport.width / 2;
		const anchorAngle = rect.centerX > viewportCenterX ? 0 : Math.PI;
		return createPoseFromSubject({
			center: { x: rect.centerX, y: rect.centerY },
			radius: subjectRadius,
			tailLength,
			anchorAngle
		});
	}
}
