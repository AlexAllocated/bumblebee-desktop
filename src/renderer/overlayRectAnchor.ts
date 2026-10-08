import { OVERLAY_RECT_ANCHORS, type OverlayRectAnchor } from "../lib/overlay";

export { OVERLAY_RECT_ANCHORS };
export type { OverlayRectAnchor };

export type OverlayRectPosition = {
	horizontalPercent: number;
	verticalPercent: number;
};

export type OverlayRectViewport = {
	width: number;
	height: number;
};

export type OverlayRectDimensions = {
	width: number;
	height: number;
};

export type OverlayRect = OverlayRectDimensions & {
	left: number;
	top: number;
	right: number;
	bottom: number;
	centerX: number;
	centerY: number;
};

export const OVERLAY_RECT_CENTER_ZONE_MIN_PERCENT = 25;
export const OVERLAY_RECT_CENTER_ZONE_MAX_PERCENT = 75;

const OVERLAY_RECT_ANCHOR_SET = new Set<OverlayRectAnchor>(OVERLAY_RECT_ANCHORS);

const finiteOr = (value: unknown, fallback: number) => {
	const parsed = Number(value);
	return Number.isFinite(parsed) ? parsed : fallback;
};

const clampNumber = (value: number, min: number, max: number) => {
	if (min <= max) return Math.min(max, Math.max(min, value));
	const midpoint = (min + max) / 2;
	return Number.isFinite(value) ? midpoint : 50;
};

export const isOverlayRectAnchor = (value: unknown): value is OverlayRectAnchor =>
	typeof value === "string" && OVERLAY_RECT_ANCHOR_SET.has(value as OverlayRectAnchor);

export const normalizeOverlayRectAnchor = (
	value: unknown,
	fallback: OverlayRectAnchor = "center"
): OverlayRectAnchor =>
	isOverlayRectAnchor(value) ? value : isOverlayRectAnchor(fallback) ? fallback : "center";

export const overlayRectFromAnchorPosition = (
	position: OverlayRectPosition,
	anchor: OverlayRectAnchor,
	dimensions: OverlayRectDimensions,
	viewport: OverlayRectViewport
): OverlayRect => {
	const anchorX = (viewport.width * position.horizontalPercent) / 100;
	const anchorY = (viewport.height * position.verticalPercent) / 100;
	let left = anchorX;
	let top = anchorY;
	if (anchor.includes("right")) {
		left = anchorX - dimensions.width;
	} else if (anchor === "center") {
		left = anchorX - dimensions.width / 2;
	}
	if (anchor.includes("bottom")) {
		top = anchorY - dimensions.height;
	} else if (anchor === "center") {
		top = anchorY - dimensions.height / 2;
	}
	const right = left + dimensions.width;
	const bottom = top + dimensions.height;
	return {
		left,
		top,
		width: dimensions.width,
		height: dimensions.height,
		right,
		bottom,
		centerX: left + dimensions.width / 2,
		centerY: top + dimensions.height / 2
	};
};

export const overlayRectPositionFromRect = (
	rect: OverlayRect,
	anchor: OverlayRectAnchor,
	viewport: OverlayRectViewport
): OverlayRectPosition => {
	const anchorPoint =
		anchor === "top-left"
			? { x: rect.left, y: rect.top }
			: anchor === "top-right"
				? { x: rect.right, y: rect.top }
				: anchor === "bottom-left"
					? { x: rect.left, y: rect.bottom }
					: anchor === "bottom-right"
						? { x: rect.right, y: rect.bottom }
						: { x: rect.centerX, y: rect.centerY };
	return {
		horizontalPercent: viewport.width ? (anchorPoint.x / viewport.width) * 100 : 0,
		verticalPercent: viewport.height ? (anchorPoint.y / viewport.height) * 100 : 0
	};
};

export const overlayRectAnchorFromPointPercent = (
	position: OverlayRectPosition,
	options?: {
		centerZoneMinPercent?: number;
		centerZoneMaxPercent?: number;
	}
): OverlayRectAnchor => {
	const centerMin = options?.centerZoneMinPercent ?? OVERLAY_RECT_CENTER_ZONE_MIN_PERCENT;
	const centerMax = options?.centerZoneMaxPercent ?? OVERLAY_RECT_CENTER_ZONE_MAX_PERCENT;
	const horizontalPercent = finiteOr(position.horizontalPercent, 50);
	const verticalPercent = finiteOr(position.verticalPercent, 50);
	if (
		horizontalPercent >= centerMin &&
		horizontalPercent <= centerMax &&
		verticalPercent >= centerMin &&
		verticalPercent <= centerMax
	) {
		return "center";
	}
	const vertical = verticalPercent < 50 ? "top" : "bottom";
	const horizontal = horizontalPercent < 50 ? "left" : "right";
	return `${vertical}-${horizontal}` as OverlayRectAnchor;
};

export const overlayRectAnchorFromRect = (
	rect: OverlayRect,
	viewport: OverlayRectViewport,
	options?: Parameters<typeof overlayRectAnchorFromPointPercent>[1]
): OverlayRectAnchor =>
	overlayRectAnchorFromPointPercent(
		{
			horizontalPercent: viewport.width ? (rect.centerX / viewport.width) * 100 : 50,
			verticalPercent: viewport.height ? (rect.centerY / viewport.height) * 100 : 50
		},
		options
	);

export const overlayRectPositionBounds = (
	anchor: OverlayRectAnchor,
	sizePercent: { widthPercent: number; heightPercent: number }
) => {
	const widthPercent = finiteOr(sizePercent.widthPercent, 0);
	const heightPercent = finiteOr(sizePercent.heightPercent, 0);
	if (anchor === "top-left") {
		return { minX: 0, maxX: 100 - widthPercent, minY: 0, maxY: 100 - heightPercent };
	}
	if (anchor === "top-right") {
		return { minX: widthPercent, maxX: 100, minY: 0, maxY: 100 - heightPercent };
	}
	if (anchor === "bottom-left") {
		return { minX: 0, maxX: 100 - widthPercent, minY: heightPercent, maxY: 100 };
	}
	if (anchor === "bottom-right") {
		return { minX: widthPercent, maxX: 100, minY: heightPercent, maxY: 100 };
	}
	return {
		minX: widthPercent / 2,
		maxX: 100 - widthPercent / 2,
		minY: heightPercent / 2,
		maxY: 100 - heightPercent / 2
	};
};

export const clampOverlayRectAnchorPosition = (
	position: OverlayRectPosition,
	anchor: OverlayRectAnchor,
	sizePercent: { widthPercent: number; heightPercent: number }
): OverlayRectPosition => {
	const bounds = overlayRectPositionBounds(anchor, sizePercent);
	return {
		horizontalPercent: clampNumber(
			finiteOr(position.horizontalPercent, 50),
			bounds.minX,
			bounds.maxX
		),
		verticalPercent: clampNumber(finiteOr(position.verticalPercent, 50), bounds.minY, bounds.maxY)
	};
};
