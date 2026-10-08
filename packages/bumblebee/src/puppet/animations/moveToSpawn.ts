import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import { Matrix, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import type { PuppetPositionManager } from "../positionManager";

const createMoveToSpawnAnimation = (
	node: TransformNode,
	positionManager: PuppetPositionManager,
	options?: { durationMs?: number; targetScale?: number }
): { group: AnimationGroup; frames: number; flipFrame: number | null } => {
	const group = new AnimationGroup("puppetMoveToSpawn", node.getScene());

	// Compute path that keeps on-screen height evolving smoothly with scale.
	// We DO NOT animate mesh scaling; instead we animate world position so the
	// apparent screen height follows s(t) between currentScale and targetScale.
	const start = node.position.clone();
	const vt = positionManager.getViewportTarget();
	const startH = vt.heightPercent ?? 0.25;
	const targetH = typeof options?.targetScale === "number" ? options.targetScale : startH;
	const occlusion = positionManager.getOcclusionRatio();

	const fps = 60;
	const durationMs = options?.durationMs ?? 1200;
	const frames = Math.max(1, Math.round((durationMs / 1000) * fps));

	const anim = new Animation(
		"puppetMovePosition",
		"position",
		fps,
		Animation.ANIMATIONTYPE_VECTOR3,
		Animation.ANIMATIONLOOPMODE_CONSTANT
	);
	const keys: { frame: number; value: Vector3 }[] = [];
	const ease = new CubicEase();
	ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
	for (let f = 0; f <= frames; f++) {
		const t = f / frames;
		const e = ease.ease(t);
		const h = startH + (targetH - startH) * e;
		const yPercent = -occlusion * h;
		const targetWorld = positionManager.getWorldPosition({ yPercent, heightPercent: h });
		const blended = Vector3.Lerp(start, targetWorld, e);
		keys.push({ frame: f, value: blended });
	}
	anim.setKeys(keys);

	group.addTargetedAnimation(anim, node);
	let flipFrame: number | null = null;
	let centerXValue: number | null = null;
	try {
		const scene = node.getScene();
		const camera = scene.activeCamera;
		const engine = scene.getEngine();
		const canvas = engine.getRenderingCanvas();
		if (camera && canvas) {
			centerXValue = canvas.clientWidth / 2;
			const viewportGlobal = camera.viewport.toGlobal(
				engine.getRenderWidth(),
				engine.getRenderHeight()
			);
			const transformMatrix = scene.getTransformMatrix();
			let prevSide: boolean | null = null;
			for (const key of keys) {
				const projected = Vector3.Project(
					key.value,
					Matrix.Identity(),
					transformMatrix,
					viewportGlobal
				);
				const x = (projected.x / engine.getRenderWidth()) * canvas.clientWidth;
				if (!Number.isFinite(x)) continue;
				const side = x >= centerXValue;
				if (prevSide !== null && side !== prevSide) {
					flipFrame = key.frame;
					break;
				}
				prevSide = side;
			}
		}
	} catch (error) {
		console.warn("Failed to compute puppet flip frame:", error);
	}
	return { group, frames, flipFrame };
};

export { createMoveToSpawnAnimation };
