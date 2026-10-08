import type { BubblePose, Point, Rect, Viewport } from "./types";

export type SubjectCircle = {
	center: Point;
	radius: number;
};

export type TrackCircle = SubjectCircle;

export type BubbleTrackPlacement = {
	pose: BubblePose;
	angle: number;
	score: number;
	rect: Rect;
	overlapRect: Rect;
	exposedTailLength: number;
	clipped: boolean;
	overlaps: boolean;
	overlapsSubject: boolean;
	tailTooShort: boolean;
};

const TAU = Math.PI * 2;

export const MIN_EXPOSED_TAIL_LENGTH_PX = 18;

export const resolveTailBaseWidth = ({
	tailLength,
	scale = 1,
	perimeterLength = Number.POSITIVE_INFINITY
}: {
	tailLength: number;
	scale?: number;
	perimeterLength?: number;
}) => {
	const baseScale = clamp(Number.isFinite(scale) ? scale : 1, 0.5, 1.5);
	const minimumWidth = 12 * baseScale;
	const maximumWidth = 56 * baseScale;
	const distanceWidth = Math.max(minimumWidth, Math.max(0, tailLength) * 0.3);
	const perimeterWidth = Number.isFinite(perimeterLength)
		? Math.max(minimumWidth, Math.max(0, perimeterLength) / 3)
		: Number.POSITIVE_INFINITY;
	return Math.min(distanceWidth, maximumWidth, perimeterWidth);
};

export const clamp = (value: number, min: number, max: number) =>
	Math.max(min, Math.min(max, value));

export const roundPoint = (point: Point): Point => ({
	x: Math.round(point.x * 100) / 100,
	y: Math.round(point.y * 100) / 100
});

export const addPoints = (a: Point, b: Point): Point => ({ x: a.x + b.x, y: a.y + b.y });

export const subtractPoints = (a: Point, b: Point): Point => ({ x: a.x - b.x, y: a.y - b.y });

export const scalePoint = (point: Point, scale: number): Point => ({
	x: point.x * scale,
	y: point.y * scale
});

export const pointDistance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);

export const vectorLength = (point: Point) => Math.hypot(point.x, point.y);

export const normalizeVector = (point: Point, fallback: Point = { x: 1, y: 0 }): Point => {
	const length = vectorLength(point);
	if (length <= 0.001) return fallback;
	return { x: point.x / length, y: point.y / length };
};

export const tailTipForPose = (pose: BubblePose): Point => addPoints(pose.anchor, pose.tailVector);

export const subjectCircleForPose = (pose: BubblePose): SubjectCircle => {
	const radius = Math.max(0, pose.subjectRadius);
	const direction = normalizeVector(pose.tailVector);
	return {
		center: addPoints(tailTipForPose(pose), scalePoint(direction, radius)),
		radius
	};
};

export const trackCircleForPose = (pose: BubblePose): TrackCircle => {
	const subject = subjectCircleForPose(pose);
	return {
		center: subject.center,
		radius: subject.radius + vectorLength(pose.tailVector)
	};
};

export const poseWithTailTip = (pose: BubblePose, tailTip: Point): BubblePose => ({
	...pose,
	tailVector: subtractPoints(tailTip, pose.anchor)
});

export const poseWithSubjectCircle = (
	pose: BubblePose,
	subject: { center: Point; radius: number }
): BubblePose => {
	const centerDistance = pointDistance(subject.center, pose.anchor);
	const radius = clamp(subject.radius, 1, Math.max(1, centerDistance - 1));
	const direction = normalizeVector(
		subtractPoints(subject.center, pose.anchor),
		normalizeVector(pose.tailVector)
	);
	const tailLength = Math.max(1, centerDistance - radius);
	return {
		...pose,
		tailVector: scalePoint(direction, tailLength),
		subjectRadius: radius
	};
};

export const poseWithSubjectCenter = (pose: BubblePose, center: Point): BubblePose =>
	poseWithSubjectCircle(pose, { center, radius: pose.subjectRadius });

export const poseWithSubjectRadius = (pose: BubblePose, radius: number): BubblePose =>
	poseWithSubjectCircle(pose, { center: subjectCircleForPose(pose).center, radius });

export const movePose = (pose: BubblePose, delta: Point): BubblePose => ({
	...pose,
	anchor: addPoints(pose.anchor, delta)
});

export const rectFromCenter = (center: Point, width: number, height: number): Rect => ({
	x: center.x - width / 2,
	y: center.y - height / 2,
	left: center.x - width / 2,
	top: center.y - height / 2,
	right: center.x + width / 2,
	bottom: center.y + height / 2,
	centerX: center.x,
	centerY: center.y,
	width,
	height
});

export const rectForSubjectCircle = (circle: SubjectCircle): Rect =>
	rectFromCenter(circle.center, circle.radius * 2, circle.radius * 2);

export const normalizeViewport = (viewport?: Viewport | null): Required<Viewport> => ({
	left: viewport?.left ?? 0,
	top: viewport?.top ?? 0,
	width:
		typeof viewport?.width === "number" && Number.isFinite(viewport.width)
			? Math.max(1, viewport.width)
			: typeof globalThis.innerWidth === "number"
				? globalThis.innerWidth
				: 1280,
	height:
		typeof viewport?.height === "number" && Number.isFinite(viewport.height)
			? Math.max(1, viewport.height)
			: typeof globalThis.innerHeight === "number"
				? globalThis.innerHeight
				: 720
});

const normalizeAngle = (angle: number) => ((angle % TAU) + TAU) % TAU;

const angleDistance = (a: number, b: number) => {
	const distance = Math.abs(normalizeAngle(a) - normalizeAngle(b));
	return Math.min(distance, TAU - distance);
};

const poseFromSubjectAngle = ({
	center,
	subjectRadius,
	tailLength,
	angle
}: {
	center: Point;
	subjectRadius: number;
	tailLength: number;
	angle: number;
}): BubblePose => {
	const direction = { x: Math.cos(angle), y: Math.sin(angle) };
	const subjectEdge = subtractPoints(center, scalePoint(direction, subjectRadius));
	const anchor = subtractPoints(center, scalePoint(direction, subjectRadius + tailLength));
	return {
		anchor: roundPoint(anchor),
		tailVector: roundPoint(subtractPoints(subjectEdge, anchor)),
		subjectRadius
	};
};

export const createPoseFromSubject = ({
	center,
	radius,
	anchorAngle = -Math.PI / 4,
	tailLength = 56
}: {
	center: Point;
	radius: number;
	anchorAngle?: number;
	tailLength?: number;
}): BubblePose =>
	poseFromSubjectAngle({
		center,
		subjectRadius: Math.max(1, radius),
		tailLength: Math.max(0, tailLength),
		angle: anchorAngle
	});

const viewportOverflow = (rect: Rect, viewport: Required<Viewport>) => {
	const minX = viewport.left + 8;
	const minY = viewport.top + 8;
	const maxX = viewport.left + viewport.width - 8;
	const maxY = viewport.top + viewport.height - 8;
	return (
		Math.max(0, minX - rect.left) +
		Math.max(0, rect.right - maxX) +
		Math.max(0, minY - rect.top) +
		Math.max(0, rect.bottom - maxY)
	);
};

const expandedRect = (rect: Rect, padding: number): Rect => ({
	x: rect.x - padding,
	y: rect.y - padding,
	left: rect.left - padding,
	top: rect.top - padding,
	right: rect.right + padding,
	bottom: rect.bottom + padding,
	centerX: rect.centerX,
	centerY: rect.centerY,
	width: rect.width + padding * 2,
	height: rect.height + padding * 2
});

const rectOverlapArea = (a: Rect, b: Rect, padding = 0) => {
	const padded = padding > 0 ? expandedRect(b, padding) : b;
	const width = Math.max(0, Math.min(a.right, padded.right) - Math.max(a.left, padded.left));
	const height = Math.max(0, Math.min(a.bottom, padded.bottom) - Math.max(a.top, padded.top));
	return width * height;
};

export const subjectClearanceFromRect = (subject: SubjectCircle, rect: Rect) => {
	const dx = Math.max(rect.left - subject.center.x, 0, subject.center.x - rect.right);
	const dy = Math.max(rect.top - subject.center.y, 0, subject.center.y - rect.bottom);
	return Math.hypot(dx, dy) - subject.radius;
};

const radiusAlongRect = (direction: Point, size: { width: number; height: number }) => {
	const halfWidth = Math.max(1, size.width / 2);
	const halfHeight = Math.max(1, size.height / 2);
	const denominator = Math.hypot(direction.x / halfWidth, direction.y / halfHeight);
	return denominator > 0.001 ? 1 / denominator : Math.min(halfWidth, halfHeight);
};

const estimatedRectForPose = (
	pose: BubblePose,
	estimatedSize: { width: number; height: number }
) => {
	const direction = normalizeVector(pose.tailVector);
	const center = subtractPoints(
		pose.anchor,
		scalePoint(direction, radiusAlongRect(direction, estimatedSize))
	);
	return rectFromCenter(center, estimatedSize.width, estimatedSize.height);
};

export const resolveTrackPlacement = ({
	pose,
	viewport,
	estimatedSize = { width: 320, height: 128 },
	overlapSize,
	occupiedRects = [],
	minimumExposedTailLength = MIN_EXPOSED_TAIL_LENGTH_PX,
	random = Math.random
}: {
	pose: BubblePose;
	viewport?: Viewport | null;
	estimatedSize?: { width: number; height: number };
	overlapSize?: { width: number; height: number };
	occupiedRects?: Rect[];
	minimumExposedTailLength?: number;
	random?: () => number;
}): BubbleTrackPlacement => {
	const safeViewport = normalizeViewport(viewport);
	const collisionSize = overlapSize ?? estimatedSize;
	const subject = subjectCircleForPose(pose);
	const minimumTailLength = Math.max(0, minimumExposedTailLength);
	const authoredTailLength = Math.max(vectorLength(pose.tailVector), minimumTailLength);
	const baseDirection = normalizeVector(pose.tailVector);
	const baseAngle = Math.atan2(baseDirection.y, baseDirection.x);
	const sampleCount = 48;
	const startAngle = normalizeAngle(baseAngle + random() * TAU);
	const candidates: BubbleTrackPlacement[] = [];
	for (let index = 0; index < sampleCount; index += 1) {
		const angle = startAngle + (TAU * index) / sampleCount;
		let tailLength = authoredTailLength;
		let candidatePose = poseFromSubjectAngle({
			center: subject.center,
			subjectRadius: subject.radius,
			tailLength,
			angle
		});
		let rect = estimatedRectForPose(candidatePose, estimatedSize);
		let exposedTailLength = subjectClearanceFromRect(subject, rect);

		// At diagonal anchors, an axis-aligned body can extend past the nominal
		// perimeter anchor. Move it farther along the same track until the whole
		// body clears the subject and leaves a useful, visible tail segment.
		for (let attempt = 0; attempt < 4 && exposedTailLength < minimumTailLength; attempt += 1) {
			tailLength += minimumTailLength - exposedTailLength + 1;
			candidatePose = poseFromSubjectAngle({
				center: subject.center,
				subjectRadius: subject.radius,
				tailLength,
				angle
			});
			rect = estimatedRectForPose(candidatePose, estimatedSize);
			exposedTailLength = subjectClearanceFromRect(subject, rect);
		}
		const overlapRect = estimatedRectForPose(candidatePose, collisionSize);
		const overflow = viewportOverflow(rect, safeViewport);
		const overlapArea = occupiedRects.reduce(
			(total, occupied) => total + rectOverlapArea(overlapRect, occupied, 12),
			0
		);
		const tailDeficit = Math.max(0, minimumTailLength - exposedTailLength);
		const overlapsSubject = exposedTailLength < 0;
		candidates.push({
			pose: candidatePose,
			angle,
			rect,
			overlapRect,
			exposedTailLength,
			clipped: overflow > 0,
			overlaps: overlapArea > 0,
			overlapsSubject,
			tailTooShort: tailDeficit > 0,
			score:
				(overlapsSubject ? 1_000_000 : 0) +
				tailDeficit * 10_000 +
				overflow * 1000 +
				overlapArea * 20 +
				angleDistance(angle, baseAngle) * 0.01
		});
	}
	const valid = candidates.filter(
		(candidate) =>
			!candidate.clipped &&
			!candidate.overlaps &&
			!candidate.overlapsSubject &&
			!candidate.tailTooShort
	);
	if (valid.length) return valid[Math.floor(random() * valid.length)] ?? valid[0]!;
	candidates.sort((a, b) => a.score - b.score);
	if (candidates[0]) return candidates[0];
	const rect = estimatedRectForPose(pose, estimatedSize);
	const exposedTailLength = subjectClearanceFromRect(subject, rect);
	return {
		pose,
		angle: baseAngle,
		rect,
		overlapRect: estimatedRectForPose(pose, collisionSize),
		exposedTailLength,
		clipped: false,
		overlaps: false,
		overlapsSubject: exposedTailLength < 0,
		tailTooShort: exposedTailLength < minimumTailLength,
		score: 0
	};
};

export const fitPointToViewport = (point: Point, viewport?: Viewport | null, margin = 8): Point => {
	const safeViewport = normalizeViewport(viewport);
	return {
		x: clamp(point.x, safeViewport.left + margin, safeViewport.left + safeViewport.width - margin),
		y: clamp(point.y, safeViewport.top + margin, safeViewport.top + safeViewport.height - margin)
	};
};
