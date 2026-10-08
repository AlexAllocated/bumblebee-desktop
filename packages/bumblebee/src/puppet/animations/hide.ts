import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import type { PuppetPositionManager } from "../positionManager";
import { bbLog } from "../../utils/bbLog";

/**
 * Slides puppet off-screen below the bottom edge.
 */
const createHideAnimation = (
	node: TransformNode,
	positionManager: PuppetPositionManager
): AnimationGroup => {
	const HIDE_FRAMES = 15; // ~250ms at 60fps
	const group = new AnimationGroup("hide", node.getScene());

	// Deterministic hide; no tracking/lerp

	// Compute scale-aware hide position (below screen)
	const currentPos = node.position.clone();
	const currentTarget = positionManager.getViewportTarget();
	const h = currentTarget.heightPercent;
	// To fully hide the puppet, ensure its TOP is below the bottom edge.
	// Top (in yPercent from bottom) = yPercent + h. Require (yPercent + h) <= 0 -> yPercent <= -h.
	// Add a small margin (10% of height) to avoid edge cases.
	const hideY = -(h * 1.1);
	const hideWorldPos = positionManager.getWorldPosition({ yPercent: hideY });
	bbLog("PHide", "init", {
		cy: +currentPos.y.toFixed(3),
		hy: +hideWorldPos.y.toFixed(3),
		hideY: +hideY.toFixed(3)
	});

	const anim = new Animation("hideAnimation", "position", 60, Animation.ANIMATIONTYPE_VECTOR3);
	anim.setKeys([
		{ frame: 0, value: currentPos },
		{ frame: HIDE_FRAMES, value: hideWorldPos }
	]);
	group.addTargetedAnimation(anim, node);

	// When the hide animation completes, keep the puppet disabled off-screen for reuse
	group.onAnimationGroupEndObservable.add(() => {
		node.setEnabled(false);
		bbLog("PHide", "end", { y: +node.position.y.toFixed(3) });
	});

	return group;
};

export { createHideAnimation };
