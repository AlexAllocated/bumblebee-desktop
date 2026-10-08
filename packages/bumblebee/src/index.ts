import "./style.css";

export { createOverlay } from "./overlay/runtime";
export { preloadBumblebeeModel } from "./bumblebee/modelAsset";
export type { BumblebeeModelPreloadOptions } from "./bumblebee/modelAsset";
export { DEFAULT_HANDLE_COLOR, resolveHandleColor } from "./utils/handleColors";
export {
	DIALOGUE_BLEEP_DISPLAY_TEXT,
	DIALOGUE_BLEEP_PLACEHOLDER,
	prepareDialogueBleeps,
	renderDialogueBleeps,
	resolveDialogueBleepCues,
	type DialogueBleepCue,
	type PreparedDialogueBleeps
} from "./overlay/dialogueBleeps";
export {
	applyBitcrusherToSamples,
	createBitcrushedAudioBuffer,
	createBitcrusherNode,
	type BitcrusherNode,
	type BitcrusherOptions,
	type BitcrusherParameters
} from "./utils/bitcrusher";
export type {
	BubbleOptions,
	BubblePresentationUpdate,
	SpeechTimeline
} from "./overlay/chatBubbles";
export type { StreamerVoiceBubblePreviewOptions } from "./overlay/streamerVoiceBubblePreview";
export type {
	ToolPresentationCompletedPayload,
	ToolPresentationContent,
	ToolPresentationFailedPayload,
	ToolPresentationImageContent,
	ToolPresentationPayload,
	ToolPresentationProgressEntry,
	ToolPresentationProgressPayload,
	ToolPresentationStartedPayload,
	ToolPresentationTool,
	ToolPresentationWebDocumentContent,
	ToolPresentationWebDocumentSource
} from "./overlay/toolPresentation";
export type {
	OverlayMediaPlayPayload,
	OverlayMediaStartPayload,
	OverlayMediaStopPayload
} from "./overlay/media";
export type {
	BubbleHandle,
	BubbleMessage,
	BubbleTarget,
	ChatPuppetOverlayOptions,
	Bumblebee,
	BumblebeeAnchorMode,
	BumblebeeEmote,
	BumblebeeOptions,
	BumblebeeScaleReference,
	BumblebeeStance,
	AdoptActorOptions,
	AdoptPuppetOptions,
	EventUnsubscribe,
	MoveScrollIntoView,
	MoveScrollIntoViewOptions,
	MoveTarget,
	NameplatePlatform,
	Overlay,
	OverlayAssetOptions,
	OverlayEvent,
	OverlayEventFor,
	OverlayEventType,
	OverlayOptions,
	OverlaySurfaceEffect,
	OverlaySurfaceOptions,
	OverlaySurfaceTarget,
	PlaceTarget,
	PositionTarget,
	Puppet,
	PuppetNameplate,
	PuppetOptions,
	PuppetPose,
	PuppetShowOptions,
	PuppetTransitionPhase,
	PuppetTransitionSoundHandler,
	Speech,
	PreparedSpeechAudio,
	SpeechAudioCue,
	SpeechBubbleFallback,
	SpeechTarget,
	SoundEffect,
	SoundEffectPlaybackOptions,
	VoiceChirpKind
} from "./overlay/types";
export type { ChatBubbleStyleTokens, NameplateStyleTokens } from "./types";
