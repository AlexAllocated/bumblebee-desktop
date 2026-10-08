export type BumblebeeTimelinePoint = {
	x: number;
	y: number;
};

export type BumblebeeTimelinePose = {
	position: BumblebeeTimelinePoint;
	scale: number;
};

export type BumblebeeTimelineEasing = "linear" | "easeInOut";

export type BumblebeeTimelineWord = {
	offsetMs: number;
	durationMs: number;
	text: string;
	textOffset?: number;
	wordLength?: number;
};

export type BumblebeeTimelineViseme = {
	offsetMs: number;
	visemeId: number;
};

export type BumblebeeSpeechTimeline = {
	visemes: BumblebeeTimelineViseme[];
	words: BumblebeeTimelineWord[];
};

export type PreparedSpeechAsset = {
	id: string;
	text: string;
	audioSrc?: string;
	audioContentType?: string;
	audioDurationMs?: number;
	timeline?: BumblebeeSpeechTimeline | null;
};

export type BumblebeeMoveAction = {
	type: "moveTo";
	id?: string;
	startMs: number;
	durationMs: number;
	from: BumblebeeTimelinePose;
	to: BumblebeeTimelinePose;
	easing?: BumblebeeTimelineEasing;
	curveIntensity?: number;
};

export type BumblebeeSpeechAction = {
	type: "say";
	id?: string;
	startMs: number;
	speech: PreparedSpeechAsset;
	bubbles?: boolean;
};

export type BumblebeeStanceAction = {
	type: "stance";
	startMs: number;
	stance: "standing" | "flying" | "hidden";
};

export type BumblebeeEmoteAction = {
	type: "emote";
	id?: string;
	startMs: number;
	durationMs: number;
	name: string;
};

export type BumblebeeTimelineAction =
	BumblebeeMoveAction | BumblebeeSpeechAction | BumblebeeStanceAction | BumblebeeEmoteAction;

export type BumblebeeTimeline = {
	version: 1;
	initialPose: BumblebeeTimelinePose;
	durationMs: number;
	actions: BumblebeeTimelineAction[];
};

export type SampledSpeechState = {
	active: boolean;
	elapsedMs: number;
	durationMs: number;
	text: string;
	revealedText: string;
	currentWord: BumblebeeTimelineWord | null;
	currentViseme: BumblebeeTimelineViseme | null;
	mouthOpen: number;
	progress: number;
};

export type BumblebeeRenderState = {
	timeMs: number;
	pose: BumblebeeTimelinePose;
	visible: boolean;
	stance: "standing" | "flying";
	talking: boolean;
	mouthOpen: number;
	activeSpeech: (SampledSpeechState & { asset: PreparedSpeechAsset }) | null;
	activeEmotes: BumblebeeEmoteAction[];
};

export type BumblebeeTimelineBuilderOptions = {
	initialPose?: Partial<BumblebeeTimelinePose> & {
		position?: Partial<BumblebeeTimelinePoint>;
	};
};

export type TimelineMoveOptions = {
	startMs?: number;
	durationMs?: number;
	easing?: BumblebeeTimelineEasing;
	curveIntensity?: number;
	id?: string;
};

export type TimelineSayOptions = {
	startMs?: number;
	wait?: boolean;
	bubbles?: boolean;
	id?: string;
};

const DEFAULT_POSE: BumblebeeTimelinePose = {
	position: { x: 0.5, y: 0.5 },
	scale: 0.55
};

const visemeMouthOpen = new Map<number, number>([
	[0, 0],
	[1, 0.95],
	[2, 1],
	[3, 0.82],
	[4, 0.68],
	[5, 0.48],
	[6, 0.34],
	[7, 0.36],
	[8, 0.58],
	[9, 0.82],
	[10, 0.66],
	[11, 0.84],
	[12, 0.24],
	[13, 0.28],
	[14, 0.32],
	[15, 0.16],
	[16, 0.24],
	[17, 0.2],
	[18, 0.18],
	[19, 0.14],
	[20, 0.12],
	[21, 0.06]
]);

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

const finiteNumber = (value: unknown, fallback: number) =>
	typeof value === "number" && Number.isFinite(value) ? value : fallback;

const normalizePoint = (
	point: Partial<BumblebeeTimelinePoint> | undefined,
	fallback: BumblebeeTimelinePoint
): BumblebeeTimelinePoint => ({
	x: finiteNumber(point?.x, fallback.x),
	y: finiteNumber(point?.y, fallback.y)
});

const normalizePose = (
	pose: Partial<BumblebeeTimelinePose> | undefined,
	fallback: BumblebeeTimelinePose
): BumblebeeTimelinePose => ({
	position: normalizePoint(pose?.position, fallback.position),
	scale: finiteNumber(pose?.scale, fallback.scale)
});

const clonePose = (pose: BumblebeeTimelinePose): BumblebeeTimelinePose => ({
	position: { ...pose.position },
	scale: pose.scale
});

const easeProgress = (progress: number, easing: BumblebeeTimelineEasing = "easeInOut") => {
	const clamped = clamp(progress, 0, 1);
	if (easing === "linear") return clamped;
	return clamped * clamped * (3 - 2 * clamped);
};

const lerp = (from: number, to: number, progress: number) => from + (to - from) * progress;

const timelineWordEndMs = (word: BumblebeeTimelineWord) =>
	word.offsetMs + Math.max(0, word.durationMs);

export const speechAssetDurationMs = (speech: PreparedSpeechAsset) => {
	if (typeof speech.audioDurationMs === "number" && Number.isFinite(speech.audioDurationMs)) {
		return Math.max(0, speech.audioDurationMs);
	}
	const timeline = speech.timeline;
	if (!timeline) return 0;
	const wordEndMs = timeline.words.reduce((max, word) => Math.max(max, timelineWordEndMs(word)), 0);
	const visemeEndMs = timeline.visemes.reduce((max, viseme) => Math.max(max, viseme.offsetMs), 0);
	return Math.max(wordEndMs, visemeEndMs);
};

export const sampleBumblebeeMove = (
	action: BumblebeeMoveAction,
	timeMs: number
): BumblebeeTimelinePose => {
	const durationMs = Math.max(1, action.durationMs);
	const progress = easeProgress((timeMs - action.startMs) / durationMs, action.easing);
	return {
		position: {
			x: lerp(action.from.position.x, action.to.position.x, progress),
			y: lerp(action.from.position.y, action.to.position.y, progress)
		},
		scale: lerp(action.from.scale, action.to.scale, progress)
	};
};

const activeWordAt = (words: BumblebeeTimelineWord[], elapsedMs: number) =>
	words.find((word) => elapsedMs >= word.offsetMs && elapsedMs < timelineWordEndMs(word)) ?? null;

const currentVisemeAt = (visemes: BumblebeeTimelineViseme[], elapsedMs: number) => {
	let current: BumblebeeTimelineViseme | null = null;
	for (const viseme of visemes) {
		if (viseme.offsetMs > elapsedMs) break;
		current = viseme;
	}
	return current;
};

const revealText = (speech: PreparedSpeechAsset, elapsedMs: number) => {
	const words = speech.timeline?.words ?? [];
	if (!words.length) return elapsedMs > 0 ? speech.text : "";

	const endedWords = words.filter((word) => word.offsetMs <= elapsedMs);
	const lastWord = endedWords.at(-1);
	if (
		lastWord &&
		typeof lastWord.textOffset === "number" &&
		typeof lastWord.wordLength === "number" &&
		Number.isFinite(lastWord.textOffset) &&
		Number.isFinite(lastWord.wordLength)
	) {
		return speech.text.slice(0, Math.max(0, lastWord.textOffset + lastWord.wordLength)).trimEnd();
	}

	return endedWords
		.map((word) => word.text)
		.join(" ")
		.trim();
};

export const sampleSpeechTimeline = (
	speech: PreparedSpeechAsset,
	elapsedMs: number
): SampledSpeechState => {
	const durationMs = speechAssetDurationMs(speech);
	const timeline = speech.timeline ?? { words: [], visemes: [] };
	const active = elapsedMs >= 0 && elapsedMs <= Math.max(durationMs, 1);
	const currentWord = active ? activeWordAt(timeline.words, elapsedMs) : null;
	const currentViseme = active ? currentVisemeAt(timeline.visemes, elapsedMs) : null;
	const mouthOpen = active ? (visemeMouthOpen.get(currentViseme?.visemeId ?? 0) ?? 0) : 0;
	return {
		active,
		elapsedMs,
		durationMs,
		text: speech.text,
		revealedText: active
			? revealText(speech, elapsedMs)
			: elapsedMs > durationMs
				? speech.text
				: "",
		currentWord,
		currentViseme,
		mouthOpen,
		progress: durationMs > 0 ? clamp(elapsedMs / durationMs, 0, 1) : active ? 1 : 0
	};
};

const actionEndMs = (action: BumblebeeTimelineAction) => {
	switch (action.type) {
		case "moveTo":
		case "emote":
			return action.startMs + action.durationMs;
		case "say":
			return action.startMs + speechAssetDurationMs(action.speech);
		case "stance":
			return action.startMs;
	}
};

const sortActions = (actions: BumblebeeTimelineAction[]) =>
	[...actions].sort((a, b) => a.startMs - b.startMs || actionEndMs(a) - actionEndMs(b));

export const createBumblebeeTimeline = (
	actions: BumblebeeTimelineAction[] = [],
	options: BumblebeeTimelineBuilderOptions = {}
): BumblebeeTimeline => {
	const initialPose = normalizePose(options.initialPose, DEFAULT_POSE);
	const sortedActions = sortActions(actions);
	const durationMs = sortedActions.reduce((max, action) => Math.max(max, actionEndMs(action)), 0);
	return {
		version: 1,
		initialPose,
		durationMs,
		actions: sortedActions
	};
};

export const sampleBumblebeeTimeline = (
	timeline: BumblebeeTimeline,
	timeMs: number
): BumblebeeRenderState => {
	const sampleTimeMs = Math.max(0, finiteNumber(timeMs, 0));
	let pose = clonePose(timeline.initialPose);
	let visible = true;
	let stance: "standing" | "flying" = "standing";
	let activeSpeech: (SampledSpeechState & { asset: PreparedSpeechAsset }) | null = null;
	const activeEmotes: BumblebeeEmoteAction[] = [];

	for (const action of timeline.actions) {
		if (action.startMs > sampleTimeMs) break;
		switch (action.type) {
			case "moveTo": {
				if (sampleTimeMs < action.startMs + action.durationMs) {
					pose = sampleBumblebeeMove(action, sampleTimeMs);
				} else {
					pose = clonePose(action.to);
				}
				break;
			}
			case "say": {
				const speech = sampleSpeechTimeline(action.speech, sampleTimeMs - action.startMs);
				if (speech.active) {
					activeSpeech = { ...speech, asset: action.speech };
				}
				break;
			}
			case "stance": {
				visible = action.stance !== "hidden";
				if (action.stance !== "hidden") stance = action.stance;
				break;
			}
			case "emote": {
				if (sampleTimeMs < action.startMs + action.durationMs) {
					activeEmotes.push(action);
				}
				break;
			}
		}
	}

	return {
		timeMs: sampleTimeMs,
		pose,
		visible,
		stance,
		talking: Boolean(activeSpeech?.active),
		mouthOpen: activeSpeech?.mouthOpen ?? 0,
		activeSpeech,
		activeEmotes
	};
};

export class BumblebeeTimelineBuilder {
	#actions: BumblebeeTimelineAction[] = [];
	#cursorMs = 0;
	#pose: BumblebeeTimelinePose;
	#initialPose: BumblebeeTimelinePose;

	constructor(options: BumblebeeTimelineBuilderOptions = {}) {
		this.#initialPose = normalizePose(options.initialPose, DEFAULT_POSE);
		this.#pose = clonePose(this.#initialPose);
	}

	get cursorMs() {
		return this.#cursorMs;
	}

	moveTo(pose: Partial<BumblebeeTimelinePose>, options: TimelineMoveOptions = {}) {
		const startMs = Math.max(0, finiteNumber(options.startMs, this.#cursorMs));
		const durationMs = Math.max(1, finiteNumber(options.durationMs, 1000));
		const to = normalizePose(pose, this.#pose);
		this.#actions.push({
			type: "moveTo",
			id: options.id,
			startMs,
			durationMs,
			from: clonePose(this.#pose),
			to,
			easing: options.easing ?? "easeInOut",
			curveIntensity: options.curveIntensity
		});
		this.#cursorMs = Math.max(this.#cursorMs, startMs + durationMs);
		this.#pose = clonePose(to);
		return this;
	}

	say(speech: PreparedSpeechAsset, options: TimelineSayOptions = {}) {
		const startMs = Math.max(0, finiteNumber(options.startMs, this.#cursorMs));
		this.#actions.push({
			type: "say",
			id: options.id,
			startMs,
			speech,
			bubbles: options.bubbles
		});
		if (options.wait ?? true) {
			this.#cursorMs = Math.max(this.#cursorMs, startMs + speechAssetDurationMs(speech));
		}
		return this;
	}

	stance(stance: BumblebeeStanceAction["stance"], startMs = this.#cursorMs) {
		this.#actions.push({
			type: "stance",
			startMs: Math.max(0, finiteNumber(startMs, this.#cursorMs)),
			stance
		});
		return this;
	}

	emote(name: string, durationMs: number, startMs = this.#cursorMs) {
		const safeStartMs = Math.max(0, finiteNumber(startMs, this.#cursorMs));
		const safeDurationMs = Math.max(1, finiteNumber(durationMs, 1));
		this.#actions.push({
			type: "emote",
			startMs: safeStartMs,
			durationMs: safeDurationMs,
			name
		});
		this.#cursorMs = Math.max(this.#cursorMs, safeStartMs + safeDurationMs);
		return this;
	}

	build(options?: { paddingMs?: number }): BumblebeeTimeline {
		const timeline = createBumblebeeTimeline(this.#actions, { initialPose: this.#initialPose });
		return {
			...timeline,
			durationMs: timeline.durationMs + Math.max(0, finiteNumber(options?.paddingMs, 0))
		};
	}
}

export const createBumblebeeTimelineBuilder = (options?: BumblebeeTimelineBuilderOptions) =>
	new BumblebeeTimelineBuilder(options);
