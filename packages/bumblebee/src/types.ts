export type PuppetRenderPoint = readonly [number, number];

export interface PuppetRenderBounds {
	left: number;
	right: number;
	top: number;
	bottom: number;
	width: number;
	height: number;
}

export interface PuppetRenderColor {
	r: number;
	g: number;
	b: number;
}

export interface PuppetRenderRibbon {
	path: PuppetRenderPoint[];
	edgeColor: PuppetRenderColor;
}

export interface PuppetSilhouetteMeasurements {
	sampleResolution: number;
	visibleArtBounds: PuppetRenderBounds;
	visibleHeight: number;
}

export interface PuppetRenderMetadata {
	version: 2;
	image: {
		width: number;
		height: number;
	};
	silhouette: PuppetSilhouetteMeasurements & {
		ribbons: PuppetRenderRibbon[];
	};
	generatedAt?: string;
}

export type PuppetRenderableMetadata = PuppetRenderMetadata | PuppetSilhouetteMeasurements;

export interface ChatBubbleStyleTokens {
	background: string;
	border: string;
	text: string;
	shadow: string;
	placeholderColor?: string;
	tail: "left" | "center" | "right" | "none";
	fontFamily: "rounded" | "comic" | "mono" | "serif";
	decoration?: "sparkles" | "rain" | "confetti" | "lava" | "halftone" | "waveform";
	motion?: "twinkle" | "drift" | "sprinkle" | "ooze" | "pulse" | "scan";
	shape:
		"rounded" | "cloud" | "caption" | "burst" | "terminal" | "ribbon" | "ellipse" | "rectangle";
}

export interface NameplateStyleTokens {
	background: string;
	border: string;
	text: string;
	accent: string;
	shadow: string;
	fontFamily: "rounded" | "comic" | "mono" | "serif";
	shape:
		| "pill"
		| "tag"
		| "banner"
		| "ticket"
		| "plaque"
		| "badge"
		| "hex"
		| "notched"
		| "ribbon"
		| "capsule";
	decoration?:
		| "shine"
		| "stitch"
		| "bolts"
		| "honeycomb"
		| "sparkle"
		| "ribbonTails"
		| "rain"
		| "confetti"
		| "lava"
		| "halftone"
		| "waveform";
	motion?:
		| "bob"
		| "pulse"
		| "flicker"
		| "swing"
		| "shimmer"
		| "pop"
		| "twinkle"
		| "drift"
		| "sprinkle"
		| "ooze"
		| "scan";
}
