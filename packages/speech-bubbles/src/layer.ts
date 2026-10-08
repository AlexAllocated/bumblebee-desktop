import { createChatDecorationRevealEvents, type ChatDecorationRevealEvent } from "./reveal";
import {
	createPoseFromSubject,
	MIN_EXPOSED_TAIL_LENGTH_PX,
	normalizeVector,
	resolveTrackPlacement,
	subjectCircleForPose,
	vectorLength
} from "./geometry";
import {
	createBubbleRenderer,
	measureBubbleBodySize,
	resolveBubbleLayoutBounds,
	type BubbleRenderer
} from "./renderer";
import { defaultBubbleStyle } from "./style";
import { PausableScheduler, type PausableTimer } from "./pausableScheduler";
import type {
	BubblePose,
	BubbleShowOptions,
	BubbleUpdateOptions,
	Rect,
	SpeechBubbleStyle,
	SpeechTimeline,
	Viewport
} from "./types";
import type { BubbleLayerOptions, BubblePageSnapshot } from "./browserTypes";

export type BubbleLayerSession = {
	id: string;
	text: string;
	visible: boolean;
	queueKey: string;
	pose: BubblePose;
	style?: SpeechBubbleStyle | null;
};

export type BubbleRevealOptions = {
	chunkIndex?: number;
	fullCursor?: number;
};

export type BubbleHideOptions = {
	immediate?: boolean;
	animate?: boolean;
};

export type BubbleLayer = {
	readonly sessions: BubbleLayerSession[];
	getPage(id: string): BubblePageSnapshot | null;
	show(options: BubbleShowOptions): void;
	reveal(id: string, text: string, options?: BubbleRevealOptions): void;
	update(id: string, options: BubbleUpdateOptions): void;
	hide(id: string, options?: BubbleHideOptions): Promise<void>;
	setPaused?(paused: boolean): void;
	dispose(): void;
};

type SessionRecord = {
	session: BubbleLayerSession;
	renderer: BubbleRenderer;
	sourceId: string;
	sourceText: string;
	timeline: SpeechTimeline | null;
	queueKey: string;
	chunkIndex: number;
	placementText: string;
	options: BubbleShowOptions;
	timers: PausableTimer[];
	revealCompletesAt: number;
	hiding: boolean;
	hideTask: Promise<void> | null;
	hideDelayTimer: PausableTimer | null;
	hideDelayResolve: (() => void) | null;
};

type BubbleRevealInput = {
	text: string;
	chunkText?: string | null;
	chunkIndex?: number | null;
	fullCursor?: number | null;
};

const BUBBLE_LINGER_MS = 1000;
const DEFAULT_Z_INDEX = 2147483000;

const defaultViewport = (): Viewport => ({
	left: 0,
	top: 0,
	width: typeof window !== "undefined" ? window.innerWidth : 1280,
	height: typeof window !== "undefined" ? window.innerHeight : 720
});

const queueKeyFor = (options: BubbleShowOptions) => options.queueKey || options.id;

const pageAudioId = (sourceId: string, chunkIndex: number) =>
	chunkIndex <= 0 ? sourceId : `${sourceId}:fit-chunk:${chunkIndex}`;

const clearTimers = (scheduler: PausableScheduler, record: SessionRecord) => {
	for (const timer of record.timers) scheduler.clearTimeout(timer);
	record.timers = [];
};

const settleHideDelay = (scheduler: PausableScheduler, record: SessionRecord) => {
	scheduler.clearTimeout(record.hideDelayTimer);
	record.hideDelayTimer = null;
	const resolve = record.hideDelayResolve;
	record.hideDelayResolve = null;
	resolve?.();
};

const waitForHideDelay = (scheduler: PausableScheduler, record: SessionRecord, delayMs: number) =>
	new Promise<void>((resolve) => {
		record.hideDelayResolve = () => {
			record.hideDelayResolve = null;
			resolve();
		};
		record.hideDelayTimer = scheduler.setTimeout(() => {
			record.hideDelayTimer = null;
			const settle = record.hideDelayResolve;
			record.hideDelayResolve = null;
			settle?.();
		}, delayMs);
	});

const revealLayoutForOptions = (options: BubbleShowOptions, viewport: Viewport) => {
	const scale = options.scale ?? 1;
	const maxWidthPercent = options.maxWidthPercent ?? 35;
	const maxHeightPercent = options.maxHeightPercent ?? 25;
	const bounds = resolveBubbleLayoutBounds({
		viewport,
		maxWidthPercent,
		maxHeightPercent,
		minWidthPx: options.minWidthPx,
		maxWidthPx: options.maxWidthPx,
		maxHeightPx: options.maxHeightPx
	});
	return {
		maxWidthPx: bounds.maxWidthPx,
		maxHeightPx: bounds.maxHeightPx,
		minWidthPx: bounds.minWidthPx,
		scale,
		shape: options.style?.shape,
		viewportWidthPx: bounds.viewport.width,
		audioDurationMs: options.audioDurationMs
	};
};

export const createBubbleLayer = (options?: BubbleLayerOptions): BubbleLayer => {
	const target = options?.target ?? (typeof document !== "undefined" ? document.body : undefined);
	if (!target) throw new Error("@hivetech/speech-bubbles requires a browser DOM.");
	const records = new Map<string, SessionRecord>();
	const scheduler = new PausableScheduler();
	let paused = false;
	const activeIdBySourceId = new Map<string, string>();
	const sourceTimers = new Map<string, PausableTimer[]>();
	const viewport = () =>
		typeof options?.viewport === "function"
			? options.viewport()
			: (options?.viewport ?? defaultViewport());
	const viewportFor = (bubbleOptions?: Pick<BubbleShowOptions, "viewport">) => {
		const bubbleViewport = bubbleOptions?.viewport;
		if (typeof bubbleViewport === "function") return bubbleViewport();
		return bubbleViewport ?? viewport();
	};

	const sourceRecords = (sourceId: string) =>
		[...records.values()].filter((record) => record.sourceId === sourceId);

	const activeRecordForSource = (sourceId: string) => {
		const activeId = activeIdBySourceId.get(sourceId);
		return (activeId ? records.get(activeId) : null) ?? records.get(sourceId) ?? null;
	};

	const pageForRecord = (record: SessionRecord): BubblePageSnapshot => ({
		id: record.session.id,
		sourceId: record.sourceId,
		text: record.session.text,
		chunkIndex: record.chunkIndex,
		element: record.renderer.element
	});

	const occupiedRectsFor = (queueKey: string, excludeId?: string) =>
		[...records.values()]
			.filter(
				(record) =>
					record.queueKey === queueKey && record.session.visible && record.session.id !== excludeId
			)
			.map((record) => record.renderer.debug?.shellRect)
			.filter((rect): rect is Rect => Boolean(rect));

	const placeRecord = (record: SessionRecord) => {
		const currentRect = record.renderer.debug?.shellRect;
		const estimatedSize = record.placementText.trim()
			? measureBubbleBodySize({
					text: record.placementText,
					style: record.options.style,
					scale: record.options.scale,
					viewport: viewportFor(record.options),
					maxWidthPercent: record.options.maxWidthPercent,
					maxHeightPercent: record.options.maxHeightPercent,
					minWidthPx: record.options.minWidthPx,
					maxWidthPx: record.options.maxWidthPx,
					maxHeightPx: record.options.maxHeightPx
				})
			: currentRect
				? { width: currentRect.width, height: currentRect.height }
				: { width: 320, height: 128 };
		const overlapSize = currentRect
			? { width: currentRect.width, height: currentRect.height }
			: estimatedSize;
		const placement = resolveTrackPlacement({
			pose: record.options.pose,
			viewport: viewportFor(record.options),
			estimatedSize,
			overlapSize,
			occupiedRects: occupiedRectsFor(record.queueKey, record.session.id)
		});
		record.session.pose = placement.pose;
		record.renderer.update({ pose: placement.pose });
	};

	const recordNeedsSubjectClearance = (record: SessionRecord) => {
		if (record.options.fixedBodyRect) return false;
		const debug = record.renderer.debug;
		return Boolean(
			debug && (debug.overlapsSubject || debug.exposedTailLength < MIN_EXPOSED_TAIL_LENGTH_PX)
		);
	};

	const createRecord = ({
		sourceId,
		id,
		text,
		placementText,
		chunkIndex,
		showOptions
	}: {
		sourceId: string;
		id: string;
		text: string;
		placementText: string;
		chunkIndex: number;
		showOptions: BubbleShowOptions;
	}) => {
		const queueKey = queueKeyFor(showOptions);
		const style = showOptions.style ?? options?.defaultStyle ?? defaultBubbleStyle;
		const renderer = createBubbleRenderer({
			id,
			target,
			text,
			pose: showOptions.pose,
			style,
			scale: showOptions.scale,
			maxWidthPercent: showOptions.maxWidthPercent,
			maxHeightPercent: showOptions.maxHeightPercent,
			minWidthPx: showOptions.minWidthPx,
			maxWidthPx: showOptions.maxWidthPx,
			maxHeightPx: showOptions.maxHeightPx,
			fixedBodySize: showOptions.fixedBodySize,
			fixedBodyRect: showOptions.fixedBodyRect,
			zIndex: (showOptions.zIndex ?? options?.zIndex ?? DEFAULT_Z_INDEX) + chunkIndex,
			placeholder: showOptions.placeholder,
			debug: showOptions.debug ?? options?.debug,
			constrainToViewport: showOptions.constrainToViewport,
			viewport: () => viewportFor(showOptions),
			scheduler
		});
		renderer.setPaused?.(paused);
		const record: SessionRecord = {
			session: {
				id,
				text,
				visible: true,
				queueKey,
				pose: showOptions.pose,
				style
			},
			renderer,
			sourceId,
			sourceText: showOptions.text,
			timeline: showOptions.timeline ?? null,
			queueKey,
			chunkIndex,
			placementText,
			options: { ...showOptions, id, text: showOptions.text, style },
			timers: [],
			revealCompletesAt: scheduler.now(),
			hiding: false,
			hideTask: null,
			hideDelayTimer: null,
			hideDelayResolve: null
		};
		placeRecord(record);
		records.set(id, record);
		activeIdBySourceId.set(sourceId, id);
		record.renderer.show();
		return record;
	};

	const clearSourceTimers = (sourceId: string) => {
		for (const timer of sourceTimers.get(sourceId) ?? []) scheduler.clearTimeout(timer);
		sourceTimers.delete(sourceId);
	};

	const hideRecord = async (
		record: SessionRecord,
		hideOptions?: BubbleHideOptions & { clearOwnedTimers?: boolean }
	) => {
		if (record.hiding) {
			if (hideOptions?.immediate) settleHideDelay(scheduler, record);
			await (record.hideTask ?? Promise.resolve());
			return;
		}
		record.hiding = true;
		if (hideOptions?.clearOwnedTimers !== false) clearTimers(scheduler, record);
		record.hideTask = (async () => {
			try {
				const remainingRevealMs = Math.max(0, record.revealCompletesAt - scheduler.now());
				const delayMs = hideOptions?.immediate ? 0 : Math.max(BUBBLE_LINGER_MS, remainingRevealMs);
				if (delayMs > 0) await waitForHideDelay(scheduler, record, delayMs);
				record.session.visible = false;
				await record.renderer.hide({ animate: hideOptions?.animate });
				record.renderer.dispose();
				const ownsRecordId = records.get(record.session.id) === record;
				if (ownsRecordId) records.delete(record.session.id);
				if (ownsRecordId && activeIdBySourceId.get(record.sourceId) === record.session.id) {
					const nextActive = sourceRecords(record.sourceId)
						.filter((entry) => entry.session.id !== record.session.id && !entry.hiding)
						.at(-1);
					if (nextActive) activeIdBySourceId.set(record.sourceId, nextActive.session.id);
					else activeIdBySourceId.delete(record.sourceId);
				}
			} finally {
				settleHideDelay(scheduler, record);
				record.hideTask = null;
			}
		})();
		await record.hideTask;
	};

	const finalRevealForChunk = (events: ChatDecorationRevealEvent[], chunkIndex: number) => {
		let finalReveal: ChatDecorationRevealEvent | null = null;
		for (const event of events) {
			if (event.kind === "reveal" && event.chunkIndex === chunkIndex) finalReveal = event;
		}
		return finalReveal;
	};

	const revealEventsForRecord = (record: SessionRecord) =>
		createChatDecorationRevealEvents(
			record.sourceText,
			record.timeline,
			revealLayoutForOptions(record.options, viewportFor(record.options))
		);

	const revealEventForCursor = (
		events: ChatDecorationRevealEvent[],
		fullCursor: number | null | undefined
	) => {
		if (typeof fullCursor !== "number" || !Number.isFinite(fullCursor)) return null;
		const cursor = Math.max(0, fullCursor);
		return (
			events.find((event) => event.kind === "reveal" && event.fullCursor === cursor) ??
			events.filter((event) => event.kind === "reveal" && event.fullCursor <= cursor).at(-1) ??
			events.find((event) => event.kind === "reveal" && event.fullCursor > cursor) ??
			null
		);
	};

	const normalizeRevealForRecord = (
		record: SessionRecord,
		reveal: BubbleRevealInput
	): Required<
		Pick<ChatDecorationRevealEvent, "text" | "chunkText" | "chunkIndex" | "fullCursor">
	> => {
		const events = revealEventsForRecord(record);
		const cursorReveal = revealEventForCursor(events, reveal.fullCursor);
		if (cursorReveal) {
			return {
				text: cursorReveal.text,
				chunkText: cursorReveal.chunkText,
				chunkIndex: cursorReveal.chunkIndex,
				fullCursor: cursorReveal.fullCursor
			};
		}

		const cleanText = reveal.text.trim();
		const chunkIndex =
			typeof reveal.chunkIndex === "number" && Number.isFinite(reveal.chunkIndex)
				? Math.max(0, Math.floor(reveal.chunkIndex))
				: 0;
		const chunkReveal = finalRevealForChunk(events, chunkIndex);
		return {
			text: cleanText,
			chunkText: reveal.chunkText?.trim() || chunkReveal?.chunkText?.trim() || cleanText,
			chunkIndex,
			fullCursor:
				typeof reveal.fullCursor === "number" && Number.isFinite(reveal.fullCursor)
					? Math.max(0, reveal.fullCursor)
					: cleanText.length
		};
	};

	const applyReveal = (sourceId: string, revealInput: BubbleRevealInput) => {
		const active = activeRecordForSource(sourceId);
		if (!active || active.hiding) return;
		const reveal = normalizeRevealForRecord(active, revealInput);
		const cleanText = reveal.text.trim();
		if (!cleanText) return;
		if (reveal.chunkIndex > active.chunkIndex) {
			try {
				options?.onPageExit?.(pageForRecord(active));
			} catch (error) {
				console.warn("Speech bubble page-exit handler failed:", error);
			}
			void hideRecord(active, {
				clearOwnedTimers: false,
				immediate: true,
				animate: active.options.animatePageExit
			});
			const id = pageAudioId(sourceId, reveal.chunkIndex);
			const existing = records.get(id);
			if (existing) {
				if (existing.hiding) return;
				activeIdBySourceId.set(sourceId, id);
				existing.placementText = reveal.chunkText?.trim() || cleanText;
				existing.renderer.update({ text: cleanText });
				existing.session.text = cleanText;
				existing.revealCompletesAt = scheduler.now();
				return;
			}
			createRecord({
				sourceId,
				id,
				text: cleanText,
				placementText: reveal.chunkText?.trim() || cleanText,
				chunkIndex: reveal.chunkIndex,
				showOptions: active.options
			});
			return;
		}
		const placementText =
			reveal.chunkText?.trim() ||
			finalRevealForChunk(revealEventsForRecord(active), active.chunkIndex)?.chunkText?.trim() ||
			cleanText;
		active.placementText = placementText;
		active.session.text = cleanText;
		active.renderer.update({ text: cleanText });
		active.revealCompletesAt = scheduler.now();
	};

	const scheduleAutoReveal = (record: SessionRecord) => {
		clearSourceTimers(record.sourceId);
		const events = createChatDecorationRevealEvents(
			record.sourceText,
			record.timeline,
			revealLayoutForOptions(record.options, viewportFor(record.options))
		);
		let elapsed = 0;
		const timers: PausableTimer[] = [];
		for (const event of events) {
			if (event.kind !== "reveal") continue;
			elapsed += Math.max(0, event.delayMs);
			const timer = scheduler.setTimeout(() => applyReveal(record.sourceId, event), elapsed);
			timers.push(timer);
		}
		sourceTimers.set(record.sourceId, timers);
		record.revealCompletesAt = scheduler.now() + elapsed;
		if (!events.length) {
			applyReveal(record.sourceId, {
				text: record.sourceText,
				chunkText: record.sourceText,
				chunkIndex: 0,
				fullCursor: record.sourceText.length
			});
			record.revealCompletesAt = scheduler.now();
		}
	};

	return {
		get sessions() {
			return [...records.values()].map((record) => ({ ...record.session }));
		},

		getPage(id) {
			const record = activeRecordForSource(id) ?? records.get(id) ?? null;
			return record ? pageForRecord(record) : null;
		},

		show(showOptions) {
			const text = showOptions.text.trim();
			if (!showOptions.id || !text) return;
			clearSourceTimers(showOptions.id);
			for (const record of sourceRecords(showOptions.id)) {
				clearTimers(scheduler, record);
				settleHideDelay(scheduler, record);
				record.renderer.dispose();
				records.delete(record.session.id);
			}
			activeIdBySourceId.delete(showOptions.id);
			const revealDriven = showOptions.autoReveal || showOptions.manualReveal;
			const firstPageText = revealDriven
				? (finalRevealForChunk(
						createChatDecorationRevealEvents(
							text,
							showOptions.timeline ?? null,
							revealLayoutForOptions(showOptions, viewportFor(showOptions))
						),
						0
					)?.chunkText.trim() ?? text)
				: text;
			const record = createRecord({
				sourceId: showOptions.id,
				id: showOptions.id,
				text: revealDriven ? "" : text,
				placementText: firstPageText,
				chunkIndex: 0,
				showOptions: { ...showOptions, text }
			});
			if (showOptions.autoReveal) scheduleAutoReveal(record);
		},

		reveal(id, text, revealOptions) {
			applyReveal(id, {
				text,
				chunkText: text,
				chunkIndex: revealOptions?.chunkIndex ?? 0,
				fullCursor: revealOptions?.fullCursor ?? text.length
			});
		},

		update(id, updateOptions) {
			const recordsToUpdate = sourceRecords(id);
			const directRecord = records.get(id);
			if (!recordsToUpdate.length && directRecord) recordsToUpdate.push(directRecord);
			for (const record of recordsToUpdate) {
				record.options = { ...record.options, ...updateOptions };
				let nextPose = updateOptions.pose ?? record.session.pose;
				if (updateOptions.pose) {
					const nextSubject = subjectCircleForPose(updateOptions.pose);
					const trackDirection = normalizeVector(record.session.pose.tailVector);
					nextPose = createPoseFromSubject({
						center: nextSubject.center,
						radius: nextSubject.radius,
						tailLength: vectorLength(record.session.pose.tailVector),
						anchorAngle: Math.atan2(trackDirection.y, trackDirection.x)
					});
				}
				record.session = {
					...record.session,
					text: updateOptions.text ?? record.session.text,
					pose: nextPose,
					style: updateOptions.style ?? record.session.style
				};
				record.renderer.update({
					...updateOptions,
					text: updateOptions.text ?? record.session.text,
					pose: nextPose
				});
				if (recordNeedsSubjectClearance(record)) placeRecord(record);
			}
		},

		async hide(id, hideOptions) {
			clearSourceTimers(id);
			const matching = sourceRecords(id);
			if (matching.length) {
				await Promise.all(matching.map((record) => hideRecord(record, hideOptions)));
				return;
			}
			const record = records.get(id);
			if (!record) return;
			await hideRecord(record, hideOptions);
		},

		setPaused(nextPaused) {
			paused = nextPaused;
			scheduler.setPaused(nextPaused);
			for (const record of records.values()) record.renderer.setPaused?.(nextPaused);
		},

		dispose() {
			for (const timers of sourceTimers.values()) {
				for (const timer of timers) scheduler.clearTimeout(timer);
			}
			sourceTimers.clear();
			for (const record of records.values()) {
				clearTimers(scheduler, record);
				settleHideDelay(scheduler, record);
				record.renderer.dispose();
			}
			records.clear();
			activeIdBySourceId.clear();
			scheduler.dispose();
		}
	};
};
