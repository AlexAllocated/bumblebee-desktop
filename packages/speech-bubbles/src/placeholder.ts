import { createBubbleRenderer, resolveBubbleLayoutBounds, type BubbleRenderer } from "./renderer";
import {
	addPoints,
	normalizeVector,
	poseWithSubjectCenter,
	poseWithSubjectRadius,
	pointDistance,
	scalePoint,
	subjectCircleForPose,
	subtractPoints,
	tailTipForPose
} from "./geometry";
import { defaultBubbleStyle } from "./style";
import type { BubbleBodyRect, BubblePose, Point } from "./types";
import type { BubblePlaceholderInteraction, BubblePlaceholderOptions } from "./browserTypes";

export type BubblePlaceholder = {
	readonly element: HTMLDivElement;
	readonly pose: BubblePose;
	update(options: Partial<Omit<BubblePlaceholderOptions, "target" | "onChange">>): void;
	dispose(): void;
};

const SVG_NS = "http://www.w3.org/2000/svg";
const SUBJECT_BODY_GAP_PX = 16;

const createSvgElement = <Tag extends keyof SVGElementTagNameMap>(tag: Tag) =>
	document.createElementNS(SVG_NS, tag);

const eventPoint = (event: PointerEvent): Point => ({ x: event.clientX, y: event.clientY });

const px = (value: number) => `${value}px`;

const applySubjectMoveHandleStyles = (element: HTMLDivElement) => {
	Object.assign(element.style, {
		position: "fixed",
		borderRadius: "999px",
		clipPath: "circle(50% at 50% 50%)",
		cursor: "move",
		pointerEvents: "auto",
		touchAction: "none"
	});
};

const applySubjectResizeHandleStyles = (element: HTMLDivElement) => {
	Object.assign(element.style, {
		position: "fixed",
		boxSizing: "border-box",
		width: "16px",
		height: "16px",
		border: "2px solid #92400e",
		borderRadius: "999px",
		background: "#fef3c7",
		boxShadow: "0 0 0 2px rgba(255, 255, 255, 0.65)",
		pointerEvents: "auto",
		transform: "translate(-50%, -50%)",
		touchAction: "none"
	});
};

const addDrag = (
	element: Element,
	onMove: (point: Point, previous: Point) => void,
	onEnd?: () => void
) => {
	let previous: Point | null = null;
	const pointerMove = (event: PointerEvent) => {
		if (!previous) return;
		const next = eventPoint(event);
		onMove(next, previous);
		previous = next;
	};
	const pointerUp = () => {
		previous = null;
		window.removeEventListener("pointermove", pointerMove);
		window.removeEventListener("pointerup", pointerUp);
		onEnd?.();
	};
	const pointerDown = (event: Event) => {
		if (!(event instanceof PointerEvent)) return;
		event.preventDefault();
		event.stopPropagation();
		previous = eventPoint(event);
		window.addEventListener("pointermove", pointerMove);
		window.addEventListener("pointerup", pointerUp, { once: true });
	};
	element.addEventListener("pointerdown", pointerDown);

	return () => {
		previous = null;
		element.removeEventListener("pointerdown", pointerDown);
		window.removeEventListener("pointermove", pointerMove);
		window.removeEventListener("pointerup", pointerUp);
	};
};

const placeholderBodySize = (options: {
	maxWidthPercent: number;
	maxHeightPercent: number;
	fixedBodySize?: { width: number; height: number };
}): { width: number; height: number } => {
	if (
		typeof options.fixedBodySize?.width === "number" &&
		Number.isFinite(options.fixedBodySize.width) &&
		typeof options.fixedBodySize.height === "number" &&
		Number.isFinite(options.fixedBodySize.height)
	) {
		return {
			width: Math.max(1, options.fixedBodySize.width),
			height: Math.max(1, options.fixedBodySize.height)
		};
	}
	const bounds = resolveBubbleLayoutBounds({
		maxWidthPercent: options.maxWidthPercent,
		maxHeightPercent: options.maxHeightPercent
	});
	return {
		width: bounds.maxWidthPx,
		height: bounds.maxHeightPx
	};
};

const cursorForCircleEdge = (point: Point, center: Point) => {
	const angle = Math.atan2(point.y - center.y, point.x - center.x);
	const sector = Math.round(angle / (Math.PI / 4) + 8) % 8;
	return (
		[
			"e-resize",
			"se-resize",
			"s-resize",
			"sw-resize",
			"w-resize",
			"nw-resize",
			"n-resize",
			"ne-resize"
		] as const
	)[sector];
};

const nearestBodyEdge = (
	rect: BubbleBodyRect,
	point: Point
): { point: Point; direction: Point; distance: number } => {
	const right = rect.left + rect.width;
	const bottom = rect.top + rect.height;
	const x = Math.max(rect.left, Math.min(right, point.x));
	const y = Math.max(rect.top, Math.min(bottom, point.y));
	const inside =
		point.x >= rect.left && point.x <= right && point.y >= rect.top && point.y <= bottom;
	if (!inside) {
		const edgePoint = { x, y };
		const direction = normalizeVector(subtractPoints(point, edgePoint));
		return { point: edgePoint, direction, distance: pointDistance(point, edgePoint) };
	}
	const distances = [
		{
			point: { x: rect.left, y: point.y },
			direction: { x: -1, y: 0 },
			distance: point.x - rect.left
		},
		{ point: { x: right, y: point.y }, direction: { x: 1, y: 0 }, distance: right - point.x },
		{
			point: { x: point.x, y: rect.top },
			direction: { x: 0, y: -1 },
			distance: point.y - rect.top
		},
		{ point: { x: point.x, y: bottom }, direction: { x: 0, y: 1 }, distance: bottom - point.y }
	];
	return distances.reduce((best, candidate) =>
		candidate.distance < best.distance ? candidate : best
	);
};

const poseFromSubjectCircle = (
	pose: BubblePose,
	subject: { center: Point; radius: number },
	bodyRect?: BubbleBodyRect
): BubblePose => {
	if (!bodyRect) {
		return poseWithSubjectRadius(poseWithSubjectCenter(pose, subject.center), subject.radius);
	}
	const radius = Math.max(1, subject.radius);
	const initialEdge = nearestBodyEdge(bodyRect, subject.center);
	const minDistance = radius + SUBJECT_BODY_GAP_PX;
	const center =
		initialEdge.distance < minDistance
			? addPoints(initialEdge.point, scalePoint(initialEdge.direction, minDistance))
			: subject.center;
	const { point: anchor, direction } = nearestBodyEdge(bodyRect, center);
	const tailTip = addPoints(center, scalePoint(direction, -radius));
	return {
		anchor,
		tailVector: subtractPoints(tailTip, anchor),
		subjectRadius: radius
	};
};

class DomBubblePlaceholder implements BubblePlaceholder {
	readonly element: HTMLDivElement;
	#svg: SVGSVGElement;
	#subject: SVGCircleElement;
	#subjectPerson: SVGGElement;
	#subjectPersonHead: SVGCircleElement;
	#subjectPersonBody: SVGPathElement;
	#subjectMoveHandle: HTMLDivElement;
	#subjectResizeHandle: HTMLDivElement;
	#renderer: BubbleRenderer;
	#disposeDrags: Array<() => void> = [];
	#options: Required<
		Pick<
			BubblePlaceholderOptions,
			"text" | "pose" | "scale" | "zIndex" | "maxWidthPercent" | "maxHeightPercent"
		>
	> &
		Pick<BubblePlaceholderOptions, "style" | "fixedBodySize" | "fixedBodyRect" | "onChange">;

	constructor(options: BubblePlaceholderOptions) {
		if (typeof document === "undefined") {
			throw new Error("@hivetech/speech-bubbles requires a browser DOM.");
		}
		const target = options.target ?? document.body;
		this.#options = {
			text: options.text ?? "Speech bubble preview",
			pose: options.pose,
			style: options.style ?? defaultBubbleStyle,
			scale: options.scale ?? 1,
			zIndex: options.zIndex ?? 2147482999,
			maxWidthPercent: options.maxWidthPercent ?? 35,
			maxHeightPercent: options.maxHeightPercent ?? 25,
			fixedBodySize: options.fixedBodySize,
			fixedBodyRect: options.fixedBodyRect,
			onChange: options.onChange
		};
		this.element = document.createElement("div");
		this.element.className = "hive-speech-bubble-placeholder";
		this.element.style.zIndex = String(this.#options.zIndex + 1);
		this.#svg = createSvgElement("svg");
		this.#svg.classList.add("hive-speech-bubble-placeholder__svg");
		this.#subject = createSvgElement("circle");
		this.#subject.classList.add("hive-speech-bubble-placeholder__subject");
		this.#subjectPerson = createSvgElement("g");
		this.#subjectPerson.classList.add("hive-speech-bubble-placeholder__person");
		this.#subjectPersonHead = createSvgElement("circle");
		this.#subjectPersonHead.classList.add("hive-speech-bubble-placeholder__person-head");
		this.#subjectPersonBody = createSvgElement("path");
		this.#subjectPersonBody.classList.add("hive-speech-bubble-placeholder__person-body");
		this.#subjectPerson.append(this.#subjectPersonBody, this.#subjectPersonHead);
		this.#subjectMoveHandle = document.createElement("div");
		this.#subjectMoveHandle.classList.add("hive-speech-bubble-placeholder__subject-move");
		applySubjectMoveHandleStyles(this.#subjectMoveHandle);
		this.#subjectResizeHandle = document.createElement("div");
		this.#subjectResizeHandle.classList.add("hive-speech-bubble-placeholder__resize-handle");
		applySubjectResizeHandleStyles(this.#subjectResizeHandle);
		this.#svg.append(this.#subject, this.#subjectPerson);
		this.element.append(this.#svg);
		target.append(this.element);
		target.append(this.#subjectMoveHandle, this.#subjectResizeHandle);
		this.#renderer = createBubbleRenderer({
			target,
			text: this.#options.text,
			pose: this.#options.pose,
			style: this.#options.style,
			scale: this.#options.scale,
			zIndex: this.#options.zIndex,
			maxWidthPercent: this.#options.maxWidthPercent,
			maxHeightPercent: this.#options.maxHeightPercent,
			fixedBodySize: placeholderBodySize(this.#options),
			fixedBodyRect: this.#options.fixedBodyRect,
			constrainToViewport: false,
			placeholder: true
		});
		this.#renderer.element.style.pointerEvents = "none";
		this.#renderer.show();
		this.#bindDragHandlers();
		this.#render();
	}

	get pose() {
		return this.#options.pose;
	}

	update(options: Partial<Omit<BubblePlaceholderOptions, "target" | "onChange">>) {
		this.#options = { ...this.#options, ...options };
		this.element.style.zIndex = String(this.#options.zIndex + 1);
		this.#subjectMoveHandle.style.zIndex = String(this.#options.zIndex + 2);
		this.#subjectResizeHandle.style.zIndex = String(this.#options.zIndex + 3);
		this.#renderer.update({
			text: this.#options.text,
			pose: this.#options.pose,
			style: this.#options.style,
			scale: this.#options.scale,
			maxWidthPercent: this.#options.maxWidthPercent,
			maxHeightPercent: this.#options.maxHeightPercent,
			zIndex: this.#options.zIndex,
			fixedBodySize: placeholderBodySize(this.#options),
			fixedBodyRect: this.#options.fixedBodyRect
		});
		this.#render();
	}

	dispose() {
		for (const disposeDrag of this.#disposeDrags) disposeDrag();
		this.#disposeDrags = [];
		this.#renderer.dispose();
		this.element.remove();
		this.#subjectMoveHandle.remove();
		this.#subjectResizeHandle.remove();
	}

	#commit(pose: BubblePose, interaction: BubblePlaceholderInteraction) {
		this.#options.pose = pose;
		this.#renderer.update({ pose });
		this.#render();
		this.#options.onChange?.(pose, interaction);
	}

	#bindDragHandlers() {
		this.#disposeDrags.push(
			addDrag(this.#subjectMoveHandle, (_next, previous) => {
				const subject = subjectCircleForPose(this.#options.pose);
				const nextSubject = {
					center: addPoints(subject.center, {
						x: _next.x - previous.x,
						y: _next.y - previous.y
					}),
					radius: subject.radius
				};
				const pose = poseFromSubjectCircle(
					this.#options.pose,
					nextSubject,
					this.#options.fixedBodyRect
				);
				this.#commit(pose, { type: "tail", tailTip: tailTipForPose(pose) });
			}),
			addDrag(this.#subjectResizeHandle, (next) => {
				const subject = subjectCircleForPose(this.#options.pose);
				const radius = Math.max(8, pointDistance(next, subject.center));
				this.#commit(
					poseFromSubjectCircle(
						this.#options.pose,
						{ ...subject, radius },
						this.#options.fixedBodyRect
					),
					{
						type: "subject",
						radius
					}
				);
			})
		);
	}

	#render() {
		const pose = this.#options.pose;
		const subject = subjectCircleForPose(pose);
		const tailTip = tailTipForPose(pose);
		this.#subjectMoveHandle.style.zIndex = String(this.#options.zIndex + 2);
		this.#subjectResizeHandle.style.zIndex = String(this.#options.zIndex + 3);
		this.#svg.setAttribute(
			"viewBox",
			`0 0 ${typeof window !== "undefined" ? window.innerWidth : 1280} ${
				typeof window !== "undefined" ? window.innerHeight : 720
			}`
		);
		this.#subject.setAttribute("cx", String(subject.center.x));
		this.#subject.setAttribute("cy", String(subject.center.y));
		this.#subject.setAttribute("r", String(subject.radius));
		this.#subjectMoveHandle.style.left = px(subject.center.x - subject.radius);
		this.#subjectMoveHandle.style.top = px(subject.center.y - subject.radius);
		this.#subjectMoveHandle.style.width = px(subject.radius * 2);
		this.#subjectMoveHandle.style.height = px(subject.radius * 2);
		const headRadius = Math.max(5, subject.radius * 0.16);
		this.#subjectPersonHead.setAttribute("cx", String(subject.center.x));
		this.#subjectPersonHead.setAttribute("cy", String(subject.center.y - subject.radius * 0.24));
		this.#subjectPersonHead.setAttribute("r", String(headRadius));
		const shoulderY = subject.center.y + subject.radius * 0.08;
		const bodyTopY = subject.center.y - subject.radius * 0.02;
		const bodyBottomY = subject.center.y + subject.radius * 0.54;
		const shoulderWidth = subject.radius * 0.86;
		const waistWidth = subject.radius * 0.42;
		this.#subjectPersonBody.setAttribute(
			"d",
			[
				`M ${subject.center.x - shoulderWidth / 2} ${bodyBottomY}`,
				`C ${subject.center.x - shoulderWidth / 2} ${shoulderY}`,
				`${subject.center.x - waistWidth / 2} ${bodyTopY}`,
				`${subject.center.x} ${bodyTopY}`,
				`C ${subject.center.x + waistWidth / 2} ${bodyTopY}`,
				`${subject.center.x + shoulderWidth / 2} ${shoulderY}`,
				`${subject.center.x + shoulderWidth / 2} ${bodyBottomY}`,
				"Z"
			].join(" ")
		);
		const oppositeDirection = subtractPoints(subject.center, tailTip);
		const oppositeLength = Math.max(1, pointDistance(subject.center, tailTip));
		const resizePoint = addPoints(
			subject.center,
			scalePoint(oppositeDirection, subject.radius / oppositeLength)
		);
		this.#subjectResizeHandle.style.left = px(resizePoint.x);
		this.#subjectResizeHandle.style.top = px(resizePoint.y);
		this.#subjectResizeHandle.style.cursor = cursorForCircleEdge(resizePoint, subject.center);
	}
}

export const createBubblePlaceholder = (options: BubblePlaceholderOptions): BubblePlaceholder =>
	new DomBubblePlaceholder(options);
