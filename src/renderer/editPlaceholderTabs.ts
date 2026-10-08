export type EditPlaceholderTabSide = "top" | "bottom" | "inside-top" | "inside-bottom";

export type EditPlaceholderTabRect = {
	left?: number;
	right?: number;
	top: number;
	bottom: number;
	width?: number;
	height: number;
};

export type EditPlaceholderTabHitRect = Required<
	Pick<EditPlaceholderTabRect, "left" | "right" | "top" | "bottom" | "height">
>;

export type EditPlaceholderTabViewport = {
	height: number;
};

type ResolveEditPlaceholderTabSideOptions = {
	marginPx?: number;
};

export const resolveEditPlaceholderTabSide = (
	rect: EditPlaceholderTabRect,
	tabHeight: number,
	viewport: EditPlaceholderTabViewport,
	{ marginPx = 4 }: ResolveEditPlaceholderTabSideOptions = {}
): EditPlaceholderTabSide => {
	if (tabHeight <= 0 || viewport.height <= 0) return "top";

	const fitsAbove = rect.top - tabHeight >= marginPx;
	if (fitsAbove) return "top";

	const fitsBelow = rect.bottom + tabHeight <= viewport.height - marginPx;
	if (fitsBelow) return "bottom";

	if (rect.height >= tabHeight + marginPx) {
		return rect.top <= marginPx ? "inside-top" : "inside-bottom";
	}

	const spaceAbove = Math.max(0, rect.top);
	const spaceBelow = Math.max(0, viewport.height - rect.bottom);
	return spaceBelow >= spaceAbove ? "bottom" : "top";
};

export const resolveEditPlaceholderTabBounds = (
	rect: Required<Pick<EditPlaceholderTabRect, "left" | "right" | "top" | "bottom">>,
	tabHeight: number,
	side: EditPlaceholderTabSide
) => {
	if (side === "top") {
		return { left: rect.left, right: rect.right, top: rect.top - tabHeight, bottom: rect.top };
	}
	if (side === "bottom") {
		return {
			left: rect.left,
			right: rect.right,
			top: rect.bottom,
			bottom: rect.bottom + tabHeight
		};
	}
	if (side === "inside-top") {
		return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.top + tabHeight };
	}
	return { left: rect.left, right: rect.right, top: rect.bottom - tabHeight, bottom: rect.bottom };
};

export const isPointInEditPlaceholderDragArea = (
	clientX: number,
	clientY: number,
	rect: EditPlaceholderTabHitRect,
	tabHeight: number,
	viewport: EditPlaceholderTabViewport
) => {
	const pointInRect = (
		bounds: Required<Pick<EditPlaceholderTabRect, "left" | "right" | "top" | "bottom">>
	) =>
		clientX >= bounds.left &&
		clientX <= bounds.right &&
		clientY >= bounds.top &&
		clientY <= bounds.bottom;

	if (pointInRect(rect)) return true;

	const tabSide = resolveEditPlaceholderTabSide(rect, tabHeight, viewport);
	const tabBounds = resolveEditPlaceholderTabBounds(rect, tabHeight, tabSide);
	return pointInRect(tabBounds);
};
