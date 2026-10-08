import type { OverlaySettings, OverlayRectAnchor } from "../lib/overlay";
type Viewport = { width: number; height: number };
export function fittedActorSize(
	viewport: Viewport,
	dimensions: { w: number; h: number },
	scale: number
) {
	const ratio = dimensions.w / dimensions.h;
	if (!viewport.width || !viewport.height || !Number.isFinite(ratio) || ratio <= 0)
		return { width: 0, height: 0 };
	if (ratio >= viewport.width / viewport.height) {
		const width = viewport.width * scale;
		return { width, height: width / ratio };
	}
	const height = viewport.height * scale;
	return { width: height * ratio, height };
}
export function bumblebeePlacement(
	settings: OverlaySettings["bumblebee"],
	viewport: Viewport,
	dimensions = { w: 1, h: 1 }
) {
	const size = fittedActorSize(viewport, dimensions, settings.scalePercentage);
	const width = (size.width * 4) / 3,
		height = (size.height * 4) / 3;
	const anchor = settings.anchor;
	const x = (viewport.width * settings.position.horizontalPercent) / 100;
	const y = (viewport.height * settings.position.verticalPercent) / 100;
	return {
		x: x + (anchor.includes("left") ? width / 2 : anchor.includes("right") ? -width / 2 : 0),
		y: y + (anchor.includes("bottom") ? 0 : anchor === "center" ? height / 2 : height)
	};
}
export function puppetRuntimePosition(
	position: { horizontalPercent: number },
	anchor: OverlayRectAnchor,
	scale: number,
	viewport: Viewport,
	aspect = 0.72,
	count = 1
) {
	const width = viewport.height * scale * aspect * Math.max(1, Math.min(10, count));
	const offset = anchor.includes("left") ? width / 2 : anchor.includes("right") ? -width / 2 : 0;
	return {
		horizontalPercent: viewport.width
			? position.horizontalPercent + (offset / viewport.width) * 100
			: 0
	};
}
