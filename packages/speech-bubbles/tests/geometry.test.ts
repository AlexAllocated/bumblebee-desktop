import { describe, expect, test } from "bun:test";
import {
	createPoseFromSubject,
	MIN_EXPOSED_TAIL_LENGTH_PX,
	movePose,
	poseWithSubjectRadius,
	poseWithTailTip,
	resolveTailBaseWidth,
	resolveTrackPlacement,
	subjectClearanceFromRect,
	subjectCircleForPose,
	tailTipForPose,
	trackCircleForPose
} from "../src";
import type { Rect } from "../src";

const rect = ({
	left,
	top,
	width,
	height
}: {
	left: number;
	top: number;
	width: number;
	height: number;
}) => ({
	x: left,
	y: top,
	left,
	top,
	right: left + width,
	bottom: top + height,
	centerX: left + width / 2,
	centerY: top + height / 2,
	width,
	height
});

const rectsOverlap = (a: Rect, b: Rect) =>
	Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left)) *
		Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top)) >
	0;

describe("speech bubble geometry", () => {
	test("widens the tail base as the exposed tail grows", () => {
		const shortBase = resolveTailBaseWidth({ tailLength: 32, perimeterLength: 600 });
		const mediumBase = resolveTailBaseWidth({ tailLength: 96, perimeterLength: 600 });
		const longBase = resolveTailBaseWidth({ tailLength: 240, perimeterLength: 600 });

		expect(shortBase).toBeLessThan(mediumBase);
		expect(mediumBase).toBeLessThan(longBase);
		expect(longBase).toBe(56);
	});

	test("derives subject and track circles from anchor, tail vector, and subject radius", () => {
		const pose = {
			anchor: { x: 100, y: 100 },
			tailVector: { x: 40, y: 0 },
			subjectRadius: 20
		};

		expect(tailTipForPose(pose)).toEqual({ x: 140, y: 100 });
		expect(subjectCircleForPose(pose)).toEqual({ center: { x: 160, y: 100 }, radius: 20 });
		expect(trackCircleForPose(pose)).toEqual({ center: { x: 160, y: 100 }, radius: 60 });
	});

	test("moves the whole pose without changing the tail vector or subject radius", () => {
		const pose = movePose(
			{ anchor: { x: 100, y: 100 }, tailVector: { x: 40, y: 0 }, subjectRadius: 20 },
			{ x: 12, y: -8 }
		);

		expect(pose).toEqual({
			anchor: { x: 112, y: 92 },
			tailVector: { x: 40, y: 0 },
			subjectRadius: 20
		});
	});

	test("updates the tail tip and subject radius independently", () => {
		const base = { anchor: { x: 100, y: 100 }, tailVector: { x: 40, y: 0 }, subjectRadius: 20 };
		expect(poseWithTailTip(base, { x: 80, y: 125 }).tailVector).toEqual({ x: -20, y: 25 });
		const resized = poseWithSubjectRadius(base, 48);
		expect(resized.subjectRadius).toBe(48);
		expect(subjectCircleForPose(resized).center).toEqual(subjectCircleForPose(base).center);
	});

	test("creates a pose around a subject circle", () => {
		const pose = createPoseFromSubject({
			center: { x: 200, y: 200 },
			radius: 40,
			tailLength: 20,
			anchorAngle: 0
		});

		expect(pose.anchor).toEqual({ x: 140, y: 200 });
		expect(pose.tailVector).toEqual({ x: 20, y: 0 });
		expect(subjectCircleForPose(pose)).toEqual({ center: { x: 200, y: 200 }, radius: 40 });
	});

	test("chooses another track point when the base placement is occupied", () => {
		const occupied = rect({ left: 70, top: 250, width: 150, height: 100 });
		const placement = resolveTrackPlacement({
			pose: createPoseFromSubject({
				center: { x: 300, y: 300 },
				radius: 40,
				tailLength: 60,
				anchorAngle: 0
			}),
			viewport: { width: 800, height: 600 },
			estimatedSize: { width: 120, height: 80 },
			occupiedRects: [occupied],
			random: () => 0
		});

		expect(placement.overlaps).toBe(false);
		expect(rectsOverlap(placement.overlapRect, occupied)).toBe(false);
		expect(rectsOverlap(placement.rect, occupied)).toBe(false);
	});

	test("checks overlap with the minimal spawn footprint instead of the full future bubble", () => {
		const occupied = rect({ left: 200, top: 260, width: 80, height: 80 });
		const placement = resolveTrackPlacement({
			pose: createPoseFromSubject({
				center: { x: 500, y: 300 },
				radius: 40,
				tailLength: 60,
				anchorAngle: 0
			}),
			viewport: { width: 800, height: 600 },
			estimatedSize: { width: 260, height: 120 },
			overlapSize: { width: 80, height: 40 },
			occupiedRects: [occupied],
			random: () => 0
		});

		expect(placement.clipped).toBe(false);
		expect(placement.overlaps).toBe(false);
		expect(rectsOverlap(placement.overlapRect, occupied)).toBe(false);
		expect(rectsOverlap(placement.rect, occupied)).toBe(true);
	});

	test("prefers placements that keep the full estimated bubble inside the viewport", () => {
		const placement = resolveTrackPlacement({
			pose: createPoseFromSubject({
				center: { x: 720, y: 520 },
				radius: 40,
				tailLength: 60,
				anchorAngle: 0
			}),
			viewport: { width: 800, height: 600 },
			estimatedSize: { width: 220, height: 110 },
			random: () => 0
		});

		expect(placement.clipped).toBe(false);
		expect(placement.rect.left).toBeGreaterThanOrEqual(8);
		expect(placement.rect.top).toBeGreaterThanOrEqual(8);
		expect(placement.rect.right).toBeLessThanOrEqual(792);
		expect(placement.rect.bottom).toBeLessThanOrEqual(592);
	});

	test("moves the bubble body clear of its subject and preserves an exposed tail", () => {
		const placement = resolveTrackPlacement({
			pose: createPoseFromSubject({
				center: { x: 400, y: 300 },
				radius: 70,
				tailLength: 2,
				anchorAngle: Math.PI / 4
			}),
			viewport: { width: 800, height: 600 },
			estimatedSize: { width: 300, height: 140 },
			random: () => 0
		});

		expect(placement.overlapsSubject).toBe(false);
		expect(placement.tailTooShort).toBe(false);
		expect(placement.exposedTailLength).toBeGreaterThanOrEqual(MIN_EXPOSED_TAIL_LENGTH_PX);
		expect(subjectClearanceFromRect(subjectCircleForPose(placement.pose), placement.rect)).toBe(
			placement.exposedTailLength
		);
	});

	test("searches the opposite side when viewport constraints crowd the subject", () => {
		const subjectCenter = { x: 90, y: 300 };
		const placement = resolveTrackPlacement({
			pose: createPoseFromSubject({
				center: subjectCenter,
				radius: 56,
				tailLength: 12,
				anchorAngle: 0
			}),
			viewport: { width: 800, height: 600 },
			estimatedSize: { width: 280, height: 120 },
			random: () => 0
		});

		expect(placement.clipped).toBe(false);
		expect(
			Math.abs(Math.atan2(Math.sin(placement.angle), Math.cos(placement.angle)))
		).toBeGreaterThan(0.5);
		expect(placement.overlapsSubject).toBe(false);
		expect(placement.exposedTailLength).toBeGreaterThanOrEqual(MIN_EXPOSED_TAIL_LENGTH_PX);
	});
});
