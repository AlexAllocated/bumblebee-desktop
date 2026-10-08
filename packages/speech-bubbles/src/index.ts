import "./style.css";
export {
	createFrameBubbleRenderer,
	sampleFrameBubble,
	seekElementAnimations,
	type FrameBubbleOptions
} from "./frameRenderer";

export { PausableScheduler, type PausableTimer } from "./pausableScheduler";

export { createBubbleLayer } from "./layer";
export { createBubblePlaceholder } from "./placeholder";
export {
	createBubbleRenderer,
	DEFAULT_BUBBLE_MIN_WIDTH_PX,
	DEFAULT_BUBBLE_MAX_WIDTH_PX
} from "./renderer";
export { bubbleCornerRadiusForShape, defaultBubbleStyle, resolveBubbleStyle } from "./style";
export {
	addPoints,
	createPoseFromSubject,
	fitPointToViewport,
	MIN_EXPOSED_TAIL_LENGTH_PX,
	movePose,
	normalizeVector,
	pointDistance,
	poseWithSubjectCenter,
	poseWithSubjectCircle,
	poseWithSubjectRadius,
	poseWithTailTip,
	rectForSubjectCircle,
	resolveTailBaseWidth,
	resolveTrackPlacement,
	scalePoint,
	subjectClearanceFromRect,
	subjectCircleForPose,
	subtractPoints,
	tailTipForPose,
	trackCircleForPose,
	vectorLength
} from "./geometry";
export {
	createChatDecorationRevealEvents as createRevealEvents,
	createChatDecorationRevealSteps as createRevealSteps
} from "./reveal";
export { chatBubbleTextMetrics, layoutEllipseText, textFitsChatBubbleLayout } from "./textLayout";
export type {
	BubbleDebug,
	BubblePose,
	BubbleShowOptions,
	BubbleUpdateOptions,
	Point,
	Rect,
	Size,
	SpeechBubbleDecoration,
	SpeechBubbleFontFamily,
	SpeechBubbleMotion,
	SpeechBubbleShape,
	SpeechBubbleStyle,
	SpeechTimeline,
	Viewport
} from "./types";
export type {
	BubbleLayerOptions,
	BubblePageSnapshot,
	BubblePlaceholderInteraction,
	BubblePlaceholderOptions
} from "./browserTypes";
export type {
	BubbleHideOptions,
	BubbleLayer,
	BubbleLayerSession,
	BubbleRevealOptions
} from "./layer";
export type { BubblePlaceholder } from "./placeholder";
export type { BubbleRenderer, BubbleRendererHideOptions, BubbleRendererOptions } from "./renderer";
export type {
	ChatDecorationRevealEvent as RevealEvent,
	ChatDecorationRevealLayout as RevealLayout,
	ChatDecorationRevealStep as RevealStep,
	ChatDecorationSpeechTimeline
} from "./reveal";
