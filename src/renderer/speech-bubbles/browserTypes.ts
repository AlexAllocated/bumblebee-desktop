import type { BubbleBodyRect, BubblePose, Point, Size, SpeechBubbleStyle, Viewport } from "./types";

export type BubbleLayerOptions = {
	target?: HTMLElement;
	zIndex?: number;
	defaultStyle?: SpeechBubbleStyle;
	debug?: boolean;
	viewport?: Viewport | (() => Viewport);
	onPageExit?: (page: BubblePageSnapshot) => void;
};

export type BubblePageSnapshot = {
	id: string;
	sourceId: string;
	text: string;
	chunkIndex: number;
	element: HTMLElement;
};

export type BubblePlaceholderOptions = {
	target?: HTMLElement;
	text?: string;
	pose: BubblePose;
	style?: SpeechBubbleStyle | null;
	scale?: number;
	zIndex?: number;
	maxWidthPercent?: number;
	maxHeightPercent?: number;
	fixedBodySize?: Size;
	fixedBodyRect?: BubbleBodyRect;
	onChange?: (pose: BubblePose, interaction: BubblePlaceholderInteraction) => void;
};

export type BubblePlaceholderInteraction =
	{ type: "tail"; tailTip: Point } | { type: "subject"; radius: number };
