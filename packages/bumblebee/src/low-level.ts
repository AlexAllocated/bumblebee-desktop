export {
	createPuppet as createPuppetModel,
	type PuppetController as PuppetModelController
} from "./puppet";
export { createWoodGrainTexture } from "./puppet/createWoodGrainTexture";
export { loadModel as loadPuppetModel } from "./puppet/loadModel";
export { renderMetadataContentKey } from "./puppet/renderMetadata";
export { registerScene, unregisterScene } from "./utils/renderManager";
export type { RenderSceneOptions } from "./utils/renderManager";
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
	resolveBootstrapBabylonPerformanceProfileName,
	type BabylonCanvasResizeOptions,
	type BabylonPerformanceObservation,
	type BabylonPerformanceProfile,
	type BabylonPerformanceProfileName,
	type PuppetConstructionSignature,
	type PuppetPerformanceOptions,
	type PuppetRenderMode
} from "./utils/babylonPerformance";
export type {
	PuppetRenderableMetadata,
	PuppetRenderMetadata,
	PuppetRenderPoint,
	PuppetSilhouetteMeasurements
} from "./types";
