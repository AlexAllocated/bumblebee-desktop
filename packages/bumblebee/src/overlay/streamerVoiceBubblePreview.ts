import { createBubbleLayer, type BubbleLayer, type BubblePose } from "@hivetech/speech-bubbles";
import type { ChatBubbleStyleTokens } from "../types";

export type StreamerVoiceBubblePreviewOptions = {
	text?: string;
	pose: BubblePose;
	style?: ChatBubbleStyleTokens | null;
	scale?: number;
	maxWidthPercent?: number;
	maxHeightPercent?: number;
	fixedBodySize?: { width: number; height: number };
	fixedBodyRect?: { left: number; top: number; width: number; height: number };
	zIndex?: number;
	debug?: boolean;
};

type StreamerVoiceBubblePreviewDependencies = {
	createLayer?: (options: { zIndex: number; debug: boolean }) => BubbleLayer;
	zIndex?: number;
	debug?: boolean;
};

const STREAMER_VOICE_BUBBLE_PREVIEW_ID = "streamer-voice-bubble:preview";
const STREAMER_VOICE_BUBBLE_QUEUE_KEY = "subject:streamerVoiceBubblePreview";
const DEFAULT_PREVIEW_TEXT = "Streamer voice bubble";
const DEFAULT_PREVIEW_Z_INDEX = 2147483012;

const finitePositive = (value: unknown): value is number =>
	typeof value === "number" && Number.isFinite(value) && value > 0;

const normalizeZIndex = (value: unknown) =>
	typeof value === "number" && Number.isFinite(value) ? Math.floor(value) : DEFAULT_PREVIEW_Z_INDEX;

const normalizeSize = (value: StreamerVoiceBubblePreviewOptions["fixedBodySize"]) =>
	finitePositive(value?.width) && finitePositive(value?.height)
		? { width: value.width, height: value.height }
		: undefined;

const normalizeRect = (value: StreamerVoiceBubblePreviewOptions["fixedBodyRect"]) =>
	typeof value?.left === "number" &&
	Number.isFinite(value.left) &&
	typeof value.top === "number" &&
	Number.isFinite(value.top) &&
	finitePositive(value.width) &&
	finitePositive(value.height)
		? { left: value.left, top: value.top, width: value.width, height: value.height }
		: undefined;

export class StreamerVoiceBubblePreview {
	#layer: BubbleLayer | null = null;
	#visible = false;
	#zIndex: number;
	#debug: boolean;
	readonly #createLayer: (options: { zIndex: number; debug: boolean }) => BubbleLayer;

	constructor(options?: StreamerVoiceBubblePreviewDependencies) {
		this.#zIndex = normalizeZIndex(options?.zIndex);
		this.#debug = options?.debug ?? false;
		this.#createLayer =
			options?.createLayer ??
			((layerOptions) =>
				createBubbleLayer({ zIndex: layerOptions.zIndex, debug: layerOptions.debug }));
	}

	show(options: StreamerVoiceBubblePreviewOptions) {
		const layer = this.#ensureLayer(options);
		const showOptions = this.#showOptions(options);
		if (this.#visible) {
			layer.update(STREAMER_VOICE_BUBBLE_PREVIEW_ID, showOptions);
			return;
		}
		layer.show(showOptions);
		this.#visible = true;
	}

	refresh(options: StreamerVoiceBubblePreviewOptions) {
		this.show(options);
	}

	async hide(options?: { immediate?: boolean }) {
		if (!this.#layer || !this.#visible) return;
		this.#visible = false;
		await this.#layer.hide(STREAMER_VOICE_BUBBLE_PREVIEW_ID, options);
	}

	async dispose() {
		await this.hide({ immediate: true });
		this.#layer?.dispose();
		this.#layer = null;
	}

	#ensureLayer(options: StreamerVoiceBubblePreviewOptions) {
		const zIndex = normalizeZIndex(options.zIndex ?? this.#zIndex);
		const debug = options.debug ?? this.#debug;
		if (this.#layer && (zIndex !== this.#zIndex || debug !== this.#debug)) {
			this.#layer.dispose();
			this.#layer = null;
			this.#visible = false;
		}
		this.#zIndex = zIndex;
		this.#debug = debug;
		this.#layer ??= this.#createLayer({ zIndex, debug });
		return this.#layer;
	}

	#showOptions(options: StreamerVoiceBubblePreviewOptions) {
		return {
			id: STREAMER_VOICE_BUBBLE_PREVIEW_ID,
			text: options.text?.trim() || DEFAULT_PREVIEW_TEXT,
			pose: options.pose,
			style: options.style ?? null,
			scale: options.scale,
			maxWidthPercent: options.maxWidthPercent,
			maxHeightPercent: options.maxHeightPercent,
			fixedBodySize: normalizeSize(options.fixedBodySize),
			fixedBodyRect: normalizeRect(options.fixedBodyRect),
			queueKey: STREAMER_VOICE_BUBBLE_QUEUE_KEY,
			placeholder: true,
			zIndex: this.#zIndex + 1,
			debug: this.#debug
		};
	}
}
