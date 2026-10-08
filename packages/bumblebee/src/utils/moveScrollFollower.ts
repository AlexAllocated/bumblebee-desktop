import type { MoveScrollIntoView } from "../overlay/types";

const DEFAULT_BLOCK_ALIGNMENT: ScrollLogicalPosition = "center";
const DEFAULT_INLINE_ALIGNMENT: ScrollLogicalPosition = "center";
const FLIGHT_HEAD_START_PROGRESS = 0.2;
const SCROLL_POSITION_EPSILON = 0.5;

type ScrollAxis = {
	current: number;
	elementStart: number;
	elementEnd: number;
	viewportSize: number;
	pageSize: number;
	alignment: ScrollLogicalPosition;
};

export type MoveScrollFollower = {
	readonly willScroll: boolean;
	update: (flightProgress: number) => void;
};

const clamp = (value: number, minimum: number, maximum: number) =>
	Math.min(maximum, Math.max(minimum, value));

const easeInOutCubic = (value: number) =>
	value < 0.5 ? 4 * value * value * value : 1 - Math.pow(-2 * value + 2, 3) / 2;

const resolveAxisDestination = ({
	current,
	elementStart,
	elementEnd,
	viewportSize,
	pageSize,
	alignment
}: ScrollAxis) => {
	let destination: number;
	switch (alignment) {
		case "start":
			destination = elementStart;
			break;
		case "end":
			destination = elementEnd - viewportSize;
			break;
		case "nearest": {
			const viewportEnd = current + viewportSize;
			if (elementStart >= current && elementEnd <= viewportEnd) return current;
			destination =
				elementStart < current ? elementStart : Math.max(elementEnd - viewportSize, elementStart);
			break;
		}
		case "center":
		default:
			destination = (elementStart + elementEnd - viewportSize) / 2;
			break;
	}
	return clamp(destination, 0, Math.max(0, pageSize - viewportSize));
};

const documentExtent = (document: Document, dimension: "scrollWidth" | "scrollHeight") =>
	Math.max(document.documentElement?.[dimension] ?? 0, document.body?.[dimension] ?? 0);

export const createMoveScrollFollower = (
	destination: HTMLElement,
	option: MoveScrollIntoView | undefined
): MoveScrollFollower | null => {
	if (!option) return null;

	const document = destination.ownerDocument;
	const view = document?.defaultView;
	if (!document || !view) return null;

	const rect = destination.getBoundingClientRect();
	const startX = view.scrollX;
	const startY = view.scrollY;
	const viewportWidth = view.innerWidth || document.documentElement.clientWidth;
	const viewportHeight = view.innerHeight || document.documentElement.clientHeight;
	const settings = option === true ? {} : option;
	const targetX = resolveAxisDestination({
		current: startX,
		elementStart: startX + rect.left,
		elementEnd: startX + rect.right,
		viewportSize: viewportWidth,
		pageSize: documentExtent(document, "scrollWidth"),
		alignment: settings.inline ?? DEFAULT_INLINE_ALIGNMENT
	});
	const targetY = resolveAxisDestination({
		current: startY,
		elementStart: startY + rect.top,
		elementEnd: startY + rect.bottom,
		viewportSize: viewportHeight,
		pageSize: documentExtent(document, "scrollHeight"),
		alignment: settings.block ?? DEFAULT_BLOCK_ALIGNMENT
	});
	const willScroll =
		Math.abs(targetX - startX) > SCROLL_POSITION_EPSILON ||
		Math.abs(targetY - startY) > SCROLL_POSITION_EPSILON;

	return {
		willScroll,
		update(flightProgress) {
			if (!willScroll || flightProgress <= FLIGHT_HEAD_START_PROGRESS) return;
			const followProgress = clamp(
				(flightProgress - FLIGHT_HEAD_START_PROGRESS) / (1 - FLIGHT_HEAD_START_PROGRESS),
				0,
				1
			);
			const easedProgress = easeInOutCubic(followProgress);
			view.scrollTo({
				left: startX + (targetX - startX) * easedProgress,
				top: startY + (targetY - startY) * easedProgress,
				behavior: "auto"
			});
		}
	};
};
