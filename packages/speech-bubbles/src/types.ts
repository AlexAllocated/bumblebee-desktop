export type Point = {
	x: number;
	y: number;
};

export type Size = {
	width: number;
	height: number;
};

export type Rect = Point &
	Size & {
		left: number;
		top: number;
		right: number;
		bottom: number;
		centerX: number;
		centerY: number;
	};

export type BubbleBodyRect = {
	left: number;
	top: number;
	width: number;
	height: number;
};

export type Viewport = {
	left?: number;
	top?: number;
	width: number;
	height: number;
};

export type SpeechBubbleShape =
	"rounded" | "cloud" | "caption" | "burst" | "terminal" | "ribbon" | "ellipse" | "rectangle";

export type SpeechBubbleFontFamily = "rounded" | "comic" | "mono" | "serif";

export type SpeechBubbleDecoration =
	"sparkles" | "rain" | "confetti" | "lava" | "halftone" | "waveform";

export type SpeechBubbleMotion = "twinkle" | "drift" | "sprinkle" | "ooze" | "pulse" | "scan";

export type SpeechBubbleStyle = {
	background: string;
	border: string;
	text: string;
	shadow: string;
	placeholderColor?: string;
	tail?: "left" | "center" | "right" | "none";
	fontFamily?: SpeechBubbleFontFamily;
	decoration?: SpeechBubbleDecoration;
	motion?: SpeechBubbleMotion;
	shape?: SpeechBubbleShape;
	borderWidthPx?: number;
	fontWeight?: number | string;
	paddingXEm?: number;
	paddingYEm?: number;
	customProperties?: Record<string, string | number>;
};

export type BubblePose = {
	anchor: Point;
	tailVector: Point;
	subjectRadius: number;
};

export type BubbleDebug = {
	contentRect: Rect;
	shellRect: Rect;
	anchor: Point;
	tailTip: Point;
	subjectCircle: { center: Point; radius: number };
	trackCircle: { center: Point; radius: number };
	exposedTailLength: number;
	overlapsSubject: boolean;
	tailVisible: boolean;
	path: string;
};

export type SpeechTimeline = {
	visemes?: Array<{ offsetMs: number; visemeId: number }>;
	words?: Array<{
		offsetMs: number;
		durationMs: number;
		text: string;
		textOffset?: number;
		wordLength?: number;
	}>;
};

export type BubbleShowOptions = {
	id: string;
	text: string;
	pose: BubblePose;
	style?: SpeechBubbleStyle | null;
	timeline?: SpeechTimeline | null;
	queueKey?: string | null;
	autoReveal?: boolean;
	manualReveal?: boolean;
	audioDurationMs?: number;
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
	animatePageExit?: boolean;
};

export type BubbleUpdateOptions = Partial<
	Pick<
		BubbleShowOptions,
		| "text"
		| "pose"
		| "style"
		| "scale"
		| "maxWidthPercent"
		| "maxHeightPercent"
		| "minWidthPx"
		| "maxWidthPx"
		| "maxHeightPx"
		| "fixedBodySize"
		| "fixedBodyRect"
		| "zIndex"
		| "debug"
		| "constrainToViewport"
		| "viewport"
	>
>;
