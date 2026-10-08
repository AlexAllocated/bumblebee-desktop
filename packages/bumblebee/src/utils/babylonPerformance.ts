import { Engine } from "@babylonjs/core/Engines/engine";
import { ScenePerformancePriority } from "@babylonjs/core/scene";
import type { AbstractEngine } from "@babylonjs/core/Engines/abstractEngine";
import type { EngineOptions } from "@babylonjs/core/Engines/thinEngine";
import type { Scene } from "@babylonjs/core/scene";

type BabylonPerformanceProfileName = "normal" | "degraded";
type PuppetRenderMode = "full" | "half" | "flat";
type PuppetPerformanceOptions = {
	renderMode: PuppetRenderMode;
	textureResolution?: number;
};
type PuppetConstructionSignature = {
	renderMode: PuppetRenderMode;
	textureResolution: number;
};
type BabylonCanvasResizeOptions = {
	minRenderPixelRatio?: number;
	maxRenderPixelRatio?: number;
	renderPixelRatio?: number;
};

type BabylonPerformanceProfile = {
	name: BabylonPerformanceProfileName;
	label: string;
	targetFPS: number;
	maxDevicePixelRatio: number;
	antialias: boolean;
	scenePerformancePriority: ScenePerformancePriority;
	carouselAnimationFPS: number;
};

type BabylonPerformanceObservation = {
	profile: BabylonPerformanceProfileName;
	averageFPS: number;
	targetFPS: number;
	sampleCount: number;
	consecutiveMisses: number;
	consecutiveHits: number;
	recommendedProfile: BabylonPerformanceProfileName;
	savedAt: number;
};

const BABYLON_PERFORMANCE_PROFILE_PARAM = "graphicsProfile";
const BABYLON_PROFILE_LEARNING_PARAM = "performanceLearning";
const BABYLON_PROFILE_BOOTSTRAP = "normal" as const;
const BABYLON_PERFORMANCE_OBSERVATION_STORAGE_KEY = "bumblebee_babylon_performance_observation_v2";
const BABYLON_PERFORMANCE_OBSERVATION_TTL_MS = 24 * 60 * 60 * 1000;
const BABYLON_PERFORMANCE_MISS_RATIO = 0.88;
const BABYLON_PERFORMANCE_HIT_RATIO = 0.97;
const BABYLON_PERFORMANCE_REQUIRED_MISSES = 3;
const BABYLON_PERFORMANCE_REQUIRED_HITS = 15;
const MAX_BABYLON_DEVICE_PIXEL_RATIO = 2;
const DEFAULT_PUPPET_TEXTURE_RESOLUTION = 1024;
const BABYLON_PERFORMANCE_PROFILE_ORDER = [
	"normal",
	"degraded"
] as const satisfies readonly BabylonPerformanceProfileName[];

const BABYLON_PERFORMANCE_PROFILES = {
	normal: {
		name: "normal",
		label: "Normal",
		targetFPS: 60,
		maxDevicePixelRatio: MAX_BABYLON_DEVICE_PIXEL_RATIO,
		antialias: true,
		scenePerformancePriority: ScenePerformancePriority.BackwardCompatible,
		carouselAnimationFPS: 60
	},
	degraded: {
		name: "degraded",
		label: "Degraded",
		targetFPS: 60,
		maxDevicePixelRatio: MAX_BABYLON_DEVICE_PIXEL_RATIO,
		antialias: false,
		scenePerformancePriority: ScenePerformancePriority.BackwardCompatible,
		carouselAnimationFPS: 20
	}
} as const satisfies Record<BabylonPerformanceProfileName, BabylonPerformanceProfile>;

type ResolveBabylonPerformanceProfileInput = {
	url?: URL | null;
	requestedProfile?: BabylonPerformanceProfileName | string | null;
	deviceMemory?: number | null;
	hardwareConcurrency?: number | null;
};

const normalizeProfileName = (value?: string | null): BabylonPerformanceProfileName | null => {
	if (!value) return null;
	const normalized = value.trim().toLowerCase();
	if (["normal", "high", "default", "60", "60fps"].includes(normalized)) {
		return "normal";
	}
	if (["degraded", "low", "slow", "software"].includes(normalized)) {
		return "degraded";
	}
	return null;
};

const getCurrentUrl = () => {
	if (typeof window === "undefined" || !window.location?.href) return null;
	try {
		return new URL(window.location.href);
	} catch {
		return null;
	}
};

const resolveBabylonPerformanceProfileName = (
	input: ResolveBabylonPerformanceProfileInput = {}
): BabylonPerformanceProfileName => {
	const requestedProfile =
		normalizeProfileName(input.requestedProfile) ??
		normalizeProfileName(input.url?.searchParams.get(BABYLON_PERFORMANCE_PROFILE_PARAM));
	if (requestedProfile) return requestedProfile;

	return "normal";
};

const resolveBabylonPerformanceProfile = (
	input?: ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null
): BabylonPerformanceProfile => {
	if (typeof input === "string") {
		return BABYLON_PERFORMANCE_PROFILES[normalizeProfileName(input) ?? "normal"];
	}
	const profileName = resolveBabylonPerformanceProfileName({
		url: input?.url ?? getCurrentUrl(),
		requestedProfile: input?.requestedProfile ?? null
	});
	return BABYLON_PERFORMANCE_PROFILES[profileName];
};

const getBabylonPerformanceProfileIndex = (profileName: BabylonPerformanceProfileName) =>
	BABYLON_PERFORMANCE_PROFILE_ORDER.indexOf(profileName);

const getNextLowerBabylonPerformanceProfileName = (
	profileName: BabylonPerformanceProfileName
): BabylonPerformanceProfileName => {
	const nextIndex = Math.min(
		BABYLON_PERFORMANCE_PROFILE_ORDER.length - 1,
		getBabylonPerformanceProfileIndex(profileName) + 1
	);
	return BABYLON_PERFORMANCE_PROFILE_ORDER[nextIndex];
};

const getNextHigherBabylonPerformanceProfileName = (
	profileName: BabylonPerformanceProfileName
): BabylonPerformanceProfileName => {
	const nextIndex = Math.max(0, getBabylonPerformanceProfileIndex(profileName) - 1);
	return BABYLON_PERFORMANCE_PROFILE_ORDER[nextIndex];
};

const isLowestBabylonPerformanceProfile = (profileName: BabylonPerformanceProfileName) =>
	getBabylonPerformanceProfileIndex(profileName) === BABYLON_PERFORMANCE_PROFILE_ORDER.length - 1;

const isHighestBabylonPerformanceProfile = (profileName: BabylonPerformanceProfileName) =>
	getBabylonPerformanceProfileIndex(profileName) === 0;

const hasForcedBabylonPerformanceProfile = (url = getCurrentUrl()) =>
	Boolean(normalizeProfileName(url?.searchParams.get(BABYLON_PERFORMANCE_PROFILE_PARAM)));

const isBabylonProfileLearningEnabled = (url = getCurrentUrl()) => {
	if (hasForcedBabylonPerformanceProfile(url)) return false;
	const value = url?.searchParams.get(BABYLON_PROFILE_LEARNING_PARAM);
	if (!value) return true;
	return !["0", "false", "off", "disabled", "no"].includes(value.trim().toLowerCase());
};

const getStoredBabylonPerformanceObservation = (
	now = Date.now()
): BabylonPerformanceObservation | null => {
	if (typeof window === "undefined") return null;
	try {
		const rawValue = window.localStorage?.getItem(BABYLON_PERFORMANCE_OBSERVATION_STORAGE_KEY);
		if (!rawValue) return null;
		const parsed = JSON.parse(rawValue) as Partial<BabylonPerformanceObservation>;
		const profile = normalizeProfileName(
			typeof parsed.profile === "string" ? parsed.profile : null
		);
		const recommendedProfile = normalizeProfileName(
			typeof parsed.recommendedProfile === "string" ? parsed.recommendedProfile : null
		);
		if (
			!profile ||
			!recommendedProfile ||
			typeof parsed.averageFPS !== "number" ||
			!Number.isFinite(parsed.averageFPS) ||
			typeof parsed.targetFPS !== "number" ||
			typeof parsed.sampleCount !== "number" ||
			typeof parsed.consecutiveMisses !== "number" ||
			typeof parsed.consecutiveHits !== "number" ||
			typeof parsed.savedAt !== "number"
		) {
			return null;
		}
		if (now - parsed.savedAt > BABYLON_PERFORMANCE_OBSERVATION_TTL_MS) return null;
		return {
			profile,
			averageFPS: parsed.averageFPS,
			targetFPS: parsed.targetFPS,
			sampleCount: parsed.sampleCount,
			consecutiveMisses: parsed.consecutiveMisses,
			consecutiveHits: parsed.consecutiveHits,
			recommendedProfile,
			savedAt: parsed.savedAt
		};
	} catch {
		return null;
	}
};

const recordBabylonPerformanceObservation = (
	profile: BabylonPerformanceProfileName,
	measuredFPS: number,
	targetFPS: number,
	now = Date.now()
): BabylonPerformanceObservation | null => {
	if (
		typeof window === "undefined" ||
		!Number.isFinite(measuredFPS) ||
		measuredFPS <= 0 ||
		!Number.isFinite(targetFPS) ||
		targetFPS <= 0
	) {
		return null;
	}
	const previous = getStoredBabylonPerformanceObservation(now);
	const continuingProfile = previous?.profile === profile;
	const averageFPS = continuingProfile
		? previous.averageFPS * 0.8 + measuredFPS * 0.2
		: measuredFPS;
	const missed = averageFPS < targetFPS * BABYLON_PERFORMANCE_MISS_RATIO;
	const hit = averageFPS >= targetFPS * BABYLON_PERFORMANCE_HIT_RATIO;
	const consecutiveMisses = missed ? (continuingProfile ? previous.consecutiveMisses : 0) + 1 : 0;
	const consecutiveHits = hit ? (continuingProfile ? previous.consecutiveHits : 0) + 1 : 0;
	let recommendedProfile = profile;
	if (profile === "normal" && consecutiveMisses >= BABYLON_PERFORMANCE_REQUIRED_MISSES) {
		recommendedProfile = "degraded";
	} else if (profile === "degraded" && consecutiveHits >= BABYLON_PERFORMANCE_REQUIRED_HITS) {
		recommendedProfile = "normal";
	}
	const observation: BabylonPerformanceObservation = {
		profile,
		averageFPS,
		targetFPS,
		sampleCount: continuingProfile ? previous.sampleCount + 1 : 1,
		consecutiveMisses,
		consecutiveHits,
		recommendedProfile,
		savedAt: now
	};
	try {
		window.localStorage?.setItem(
			BABYLON_PERFORMANCE_OBSERVATION_STORAGE_KEY,
			JSON.stringify(observation)
		);
	} catch {
		// Storage is a performance hint only; hardened browser settings should not affect rendering.
	}
	return observation;
};

const resolveBootstrapBabylonPerformanceProfileName = (
	input: Pick<ResolveBabylonPerformanceProfileInput, "deviceMemory" | "hardwareConcurrency"> = {}
): BabylonPerformanceProfileName => {
	const detectedDeviceMemory =
		typeof navigator === "undefined"
			? undefined
			: (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
	const detectedHardwareConcurrency =
		typeof navigator === "undefined" ? undefined : navigator.hardwareConcurrency;
	const deviceMemory = input.deviceMemory === undefined ? detectedDeviceMemory : input.deviceMemory;
	const hardwareConcurrency =
		input.hardwareConcurrency === undefined
			? detectedHardwareConcurrency
			: input.hardwareConcurrency;
	if (typeof deviceMemory === "number" && deviceMemory <= 4) return "degraded";
	if (typeof hardwareConcurrency === "number" && hardwareConcurrency <= 2) {
		return "degraded";
	}
	return BABYLON_PROFILE_BOOTSTRAP;
};

const resolveInitialBabylonPerformanceProfileName = (
	input: ResolveBabylonPerformanceProfileInput = {}
): BabylonPerformanceProfileName => {
	const requestedProfile =
		normalizeProfileName(input.requestedProfile) ??
		normalizeProfileName(input.url?.searchParams.get(BABYLON_PERFORMANCE_PROFILE_PARAM));
	if (requestedProfile) return requestedProfile;
	if (isBabylonProfileLearningEnabled(input.url ?? getCurrentUrl())) {
		return (
			getStoredBabylonPerformanceObservation()?.recommendedProfile ??
			resolveBootstrapBabylonPerformanceProfileName(input)
		);
	}
	return "normal";
};

const resolveInitialBabylonPerformanceProfile = (
	input?: ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null
): BabylonPerformanceProfile => {
	if (typeof input === "string") {
		return BABYLON_PERFORMANCE_PROFILES[normalizeProfileName(input) ?? "normal"];
	}
	const profileName = resolveInitialBabylonPerformanceProfileName({
		url: input?.url ?? getCurrentUrl(),
		requestedProfile: input?.requestedProfile ?? null,
		deviceMemory: input?.deviceMemory,
		hardwareConcurrency: input?.hardwareConcurrency
	});
	return BABYLON_PERFORMANCE_PROFILES[profileName];
};

const applyBabylonPerformanceProfile = (
	engine: AbstractEngine,
	scene: Scene | null | undefined,
	profileInput?:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null
) => {
	const profile = resolveBabylonPerformanceProfile(profileInput);
	engine.maxFPS = profile.targetFPS;
	if (scene) {
		scene.performancePriority = profile.scenePerformancePriority;
		scene.skipPointerMovePicking = true;
	}
	return profile;
};

const normalizeRenderPixelRatio = (value?: number | null) =>
	typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;

const resolveRenderPixelRatio = (
	profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	devicePixelRatio: number,
	options?: BabylonCanvasResizeOptions
) => {
	const profile = resolveBabylonPerformanceProfile(profileInput);
	const safeDevicePixelRatio = Math.max(0.25, devicePixelRatio || 1);
	const maxRenderPixelRatio =
		normalizeRenderPixelRatio(options?.maxRenderPixelRatio) ?? profile.maxDevicePixelRatio;
	let renderPixelRatio =
		normalizeRenderPixelRatio(options?.renderPixelRatio) ??
		Math.min(safeDevicePixelRatio, maxRenderPixelRatio);

	const minRenderPixelRatio = normalizeRenderPixelRatio(options?.minRenderPixelRatio);
	if (minRenderPixelRatio !== null) {
		renderPixelRatio = Math.max(renderPixelRatio, minRenderPixelRatio);
	}

	if (maxRenderPixelRatio !== null) {
		renderPixelRatio = Math.min(renderPixelRatio, maxRenderPixelRatio);
	}

	return Math.max(0.25, renderPixelRatio);
};

const resolveHardwareScalingLevel = (
	profileInput?:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	devicePixelRatio = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1,
	options?: BabylonCanvasResizeOptions
) => {
	const safeDevicePixelRatio = Math.max(0.25, devicePixelRatio || 1);
	const renderPixelRatio = resolveRenderPixelRatio(
		profileInput ?? null,
		safeDevicePixelRatio,
		options
	);
	return 1 / renderPixelRatio;
};

const resizeBabylonCanvas = (
	engine: Engine,
	canvas: HTMLCanvasElement,
	width: number,
	height: number,
	profileInput?:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	options?: BabylonCanvasResizeOptions
) => {
	const devicePixelRatio = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
	const scalingLevel = resolveHardwareScalingLevel(profileInput, devicePixelRatio, options);
	const renderPixelRatio = devicePixelRatio / scalingLevel;
	const renderWidth = Math.max(1, Math.round(width * renderPixelRatio));
	const renderHeight = Math.max(1, Math.round(height * renderPixelRatio));

	if (canvas.width !== renderWidth) canvas.width = renderWidth;
	if (canvas.height !== renderHeight) canvas.height = renderHeight;
	canvas.style.width = `${Math.max(width, 1)}px`;
	canvas.style.height = `${Math.max(height, 1)}px`;

	engine.setHardwareScalingLevel(scalingLevel);
	engine.resize();
};

const createBabylonEngine = (
	canvas: HTMLCanvasElement,
	profileInput?:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	options?: EngineOptions,
	adaptToDeviceRatio = true
) => {
	const profile = resolveBabylonPerformanceProfile(profileInput);
	const engine = new Engine(canvas, profile.antialias, options ?? {}, adaptToDeviceRatio);
	applyBabylonPerformanceProfile(engine, null, profile.name);
	return engine;
};

const runProfiledRenderLoop = (
	engine: Engine,
	profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	render: () => void
) => {
	const profile = applyBabylonPerformanceProfile(engine, null, profileInput);
	let lastRenderTime = 0;
	const frameInterval = 1000 / profile.targetFPS;
	const loop = () => {
		const now = performance.now();
		const delta = now - lastRenderTime;
		if (delta < frameInterval) return;
		lastRenderTime = now - (delta % frameInterval);
		render();
	};
	engine.runRenderLoop(loop);
	return loop;
};

const resolvePuppetRenderMode = (
	_profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	_requested: PuppetRenderMode = "flat"
): PuppetRenderMode => "flat";

const resolvePuppetTextureResolution = (
	profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	requested?: number
) => {
	resolveBabylonPerformanceProfile(profileInput);
	return requested;
};

const resolveEffectivePuppetTextureResolution = (options: PuppetPerformanceOptions) =>
	options.textureResolution ?? DEFAULT_PUPPET_TEXTURE_RESOLUTION;

const resolvePuppetPerformanceOptions = (
	profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	requestedTextureResolution?: number,
	requestedRenderMode: PuppetRenderMode = "flat"
): PuppetPerformanceOptions => ({
	renderMode: resolvePuppetRenderMode(profileInput, requestedRenderMode),
	textureResolution: resolvePuppetTextureResolution(profileInput, requestedTextureResolution)
});

const resolvePuppetConstructionSignature = (
	profileInput:
		ResolveBabylonPerformanceProfileInput | BabylonPerformanceProfileName | string | null,
	requestedTextureResolution?: number,
	requestedRenderMode: PuppetRenderMode = "flat"
): PuppetConstructionSignature => {
	const options = resolvePuppetPerformanceOptions(
		profileInput,
		requestedTextureResolution,
		requestedRenderMode
	);
	return {
		renderMode: options.renderMode,
		textureResolution: resolveEffectivePuppetTextureResolution(options)
	};
};

const readPuppetConstructionSignatureFromMetadata = (
	metadata?: Record<string, unknown> | null
): PuppetConstructionSignature | null => {
	const renderMode = metadata?.renderMode;
	const effectiveTextureResolution = metadata?.effectiveTextureResolution;
	if (renderMode !== "full" && renderMode !== "half" && renderMode !== "flat") return null;
	if (typeof effectiveTextureResolution !== "number") return null;
	return { renderMode, textureResolution: effectiveTextureResolution };
};

const puppetConstructionSignaturesMatch = (
	a: PuppetConstructionSignature | null | undefined,
	b: PuppetConstructionSignature | null | undefined
) =>
	Boolean(a && b && a.renderMode === b.renderMode && a.textureResolution === b.textureResolution);

export type {
	BabylonCanvasResizeOptions,
	BabylonPerformanceObservation,
	BabylonPerformanceProfile,
	BabylonPerformanceProfileName,
	PuppetConstructionSignature,
	PuppetPerformanceOptions,
	PuppetRenderMode
};
export {
	BABYLON_PERFORMANCE_OBSERVATION_STORAGE_KEY,
	BABYLON_PERFORMANCE_PROFILE_PARAM,
	BABYLON_PROFILE_BOOTSTRAP,
	BABYLON_PROFILE_LEARNING_PARAM,
	BABYLON_PERFORMANCE_PROFILE_ORDER,
	BABYLON_PERFORMANCE_PROFILES,
	DEFAULT_PUPPET_TEXTURE_RESOLUTION,
	applyBabylonPerformanceProfile,
	createBabylonEngine,
	getStoredBabylonPerformanceObservation,
	getNextHigherBabylonPerformanceProfileName,
	getNextLowerBabylonPerformanceProfileName,
	hasForcedBabylonPerformanceProfile,
	isHighestBabylonPerformanceProfile,
	isBabylonProfileLearningEnabled,
	isLowestBabylonPerformanceProfile,
	resizeBabylonCanvas,
	resolveBabylonPerformanceProfile,
	resolveBabylonPerformanceProfileName,
	resolveHardwareScalingLevel,
	resolveInitialBabylonPerformanceProfile,
	resolveInitialBabylonPerformanceProfileName,
	resolveEffectivePuppetTextureResolution,
	resolvePuppetConstructionSignature,
	resolvePuppetPerformanceOptions,
	resolvePuppetRenderMode,
	resolvePuppetTextureResolution,
	readPuppetConstructionSignatureFromMetadata,
	runProfiledRenderLoop,
	puppetConstructionSignaturesMatch,
	recordBabylonPerformanceObservation,
	resolveBootstrapBabylonPerformanceProfileName
};
