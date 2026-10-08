import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { PuppetPositionManager, PuppetPosition } from "../positionManager";
import { bbLog } from "../../utils/bbLog";

const applyShowNaturalRoll = (
	node: TransformNode,
	positionManager: PuppetPositionManager,
	options?: { disableRoll?: boolean }
) => {
	const randomRoll = getShowNaturalRoll(positionManager, options);
	if (randomRoll === null) return;

	const rollQ = Quaternion.RotationAxis(Vector3.Forward(), randomRoll);
	node.rotationQuaternion = (node.rotationQuaternion ?? Quaternion.Identity()).multiply(rollQ);
	bbLog("PShow", "roll", { roll: +randomRoll.toFixed(3) });
};

const getShowNaturalRollQuaternion = (
	base: Quaternion,
	positionManager: PuppetPositionManager,
	options?: { disableRoll?: boolean }
) => {
	const randomRoll = getShowNaturalRoll(positionManager, options);
	if (randomRoll === null) return base.clone();
	bbLog("PShow", "roll", { roll: +randomRoll.toFixed(3) });
	return base.multiply(Quaternion.RotationAxis(Vector3.Forward(), randomRoll));
};

const getShowNaturalRoll = (
	positionManager: PuppetPositionManager,
	options?: { disableRoll?: boolean }
) => {
	if (options?.disableRoll) return null;

	// Apply natural rotation for bottom edge entry.
	// Calculate safe rotation range based on distance from edges.
	const { xPercent, heightPercent: puppetHeightPercent } = positionManager.getViewportTarget();

	let availableSpace: number;
	if (xPercent <= 0.5) {
		availableSpace = xPercent - 0.02;
	} else {
		availableSpace = 1.0 - xPercent - 0.02;
	}

	const puppetWidthPercent = puppetHeightPercent * 0.2;
	const horizontalSpaceForRotation = availableSpace - puppetWidthPercent;
	const maxSafeRotation = Math.max(0, horizontalSpaceForRotation / puppetHeightPercent);
	const conservativeSafeRotation = maxSafeRotation * 0.6;
	const cappedSafeRotation = Math.min(conservativeSafeRotation, Math.PI / 6);

	const MAX_DESIRED_ROTATION = (Math.PI * 10) / 180;
	const ROTATION_RANGE = Math.min(MAX_DESIRED_ROTATION, cappedSafeRotation);
	const MAX_DESIRED_ROLL = (Math.PI * 10) / 180;
	const rollRange = Math.min(MAX_DESIRED_ROLL, ROTATION_RANGE);
	const randomValue = Math.random();
	return (randomValue * 2 - 1) * rollRange;
};

/**
 * Slides a puppet in from bottom edge with natural rotation.
 * Uses position manager for consistent viewport-relative positioning.
 */
const createShowAnimation = (
	node: TransformNode,
	positionManager: PuppetPositionManager,
	puppetPosition?: PuppetPosition,
	options?: { disableRoll?: boolean }
): AnimationGroup => {
	const SHOW_FRAMES = 24; // 400ms at 60fps: readable without delaying the speaker's entrance.
	const group = new AnimationGroup("show", node.getScene());

	// No tracking/lerp; we animate deterministically.

	// If a puppetPosition is provided for this show, apply it; otherwise, honor
	// whatever the position manager currently holds (set elsewhere by caller).
	if (puppetPosition) {
		positionManager.setPuppetPosition(puppetPosition);
	}

	// Compute scale-aware rest and start positions
	const currentTarget = positionManager.getViewportTarget();
	const h = currentTarget.heightPercent;
	const occlusion = positionManager.getOcclusionRatio();
	const restY = -occlusion * h; // keep portion of puppet height below the bottom edge
	// Start fully off-screen: require top (yPercent + h) <= 0
	// => yPercent <= -h; add a margin based on occlusion
	const startY = -(h * (1 + occlusion));

	// Get start position (below screen) and end position (target)
	const startWorldPos = positionManager.getWorldPosition({ yPercent: startY });
	const endWorldPos = positionManager.getWorldPosition({ yPercent: restY });
	bbLog("PShow", "init", {
		h: +h.toFixed(3),
		restY: +restY.toFixed(3),
		startY: +startY.toFixed(3),
		sy: +startWorldPos.y.toFixed(3),
		ey: +endWorldPos.y.toFixed(3)
	});

	// Set initial position
	node.position.copyFrom(startWorldPos);

	// Use perspective scaling instead of mesh scaling
	node.scaling.setAll(1.0);

	// Apply a subtle Z-axis tilt (roll) for organic entry, ±10° capped by space.
	applyShowNaturalRoll(node, positionManager, options);

	// Create position animation
	const anim = new Animation(
		"showAnimation",
		"position",
		60,
		Animation.ANIMATIONTYPE_VECTOR3,
		Animation.ANIMATIONLOOPMODE_CONSTANT
	);
	anim.setKeys([
		{ frame: 0, value: startWorldPos.clone() },
		{ frame: SHOW_FRAMES, value: endWorldPos.clone() }
	]);
	const ease = new CubicEase();
	ease.setEasingMode(EasingFunction.EASINGMODE_EASEOUT);
	anim.setEasingFunction(ease);
	group.addTargetedAnimation(anim, node);

	// No hard snap: rely on playAnimation strictEnd to finish precisely at last key

	return group;
};

export { applyShowNaturalRoll, createShowAnimation, getShowNaturalRollQuaternion };
