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
